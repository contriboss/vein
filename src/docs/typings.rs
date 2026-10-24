//! An npm package's API as its TypeScript declarations: the `.d.ts` files
//! in its tarball, the one `package.json` names first.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use serde_json::Value;

use super::archive::read_text;
use super::{PAGE_LIMIT, cut};

/// The most read from all of a package's declaration files.
const TOTAL_LIMIT: usize = 16 << 20;
/// Other declaration files named on the main file's page.
const LISTED: usize = 40;

/// A package's declaration files, by path below its root.
#[derive(Debug, Default)]
pub struct Typings {
    /// What `package.json` names: `types`, `typings`, or `exports["."].types`.
    pub declared: Option<String>,
    pub files: BTreeMap<String, String>,
}

impl Typings {
    /// The file a reader starts at: the declared one, else `index.d.ts`,
    /// else the first there is.
    #[must_use]
    pub fn main(&self) -> Option<&str> {
        self.declared
            .as_deref()
            .map(|path| path.trim_start_matches("./"))
            .filter(|path| self.files.contains_key(*path))
            .or_else(|| {
                self.files
                    .contains_key("index.d.ts")
                    .then_some("index.d.ts")
            })
            .or_else(|| self.files.keys().next().map(String::as_str))
    }
}

/// The declaration files of an npm tarball (`package/…`).
pub fn read(tarball: &[u8]) -> Result<Typings> {
    let mut archive = tar::Archive::new(GzDecoder::new(tarball));
    let mut typings = Typings::default();
    let mut total = 0;
    for entry in archive.entries().context("reading the npm tarball")? {
        let entry = entry.context("reading the npm tarball")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .context("reading the npm tarball")?
            .into_owned();
        let Some(relative) = path
            .to_str()
            .and_then(|p| p.split_once('/'))
            .map(|(_, rest)| rest.to_owned())
        else {
            continue;
        };
        if relative.starts_with("node_modules/") {
            continue;
        }
        if relative == "package.json" {
            let manifest: Value = serde_json::from_str(&read_text(entry)?).unwrap_or(Value::Null);
            typings.declared = declared(&manifest);
        } else if relative.ends_with(".d.ts")
            || relative.ends_with(".d.mts")
            || relative.ends_with(".d.cts")
        {
            if total >= TOTAL_LIMIT {
                continue;
            }
            let text = read_text(entry)?;
            total += text.len();
            typings.files.insert(relative, text);
        }
    }
    Ok(typings)
}

/// `types`, `typings`, or the `types` of `exports["."]`.
fn declared(manifest: &Value) -> Option<String> {
    let field = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_owned);
    field(manifest.get("types"))
        .or_else(|| field(manifest.get("typings")))
        .or_else(|| field(manifest.pointer("/exports/./types")))
}

/// The page for `file` (a path below the package's root), or the main one.
///
/// The main declaration file's page lists the others; `None` when there is
/// no such file. `heading` names the package and where the declarations
/// come from.
#[must_use]
pub fn render(typings: &Typings, file: Option<&str>, heading: &str) -> Option<String> {
    let shown = match file {
        Some(file) => file.trim_start_matches("./"),
        None => typings.main()?,
    };
    let text = typings.files.get(shown)?;
    let mut page = format!("# {heading}\n\n`{shown}`");
    if typings
        .declared
        .as_deref()
        .map(|d| d.trim_start_matches("./"))
        == Some(shown)
    {
        page.push_str(", as `package.json` declares");
    }
    page.push_str("\n\n");
    if file.is_none() && typings.files.len() > 1 {
        // @types/lodash has 705: name the first few.
        let others: Vec<&String> = typings
            .files
            .keys()
            .filter(|path| path.as_str() != shown)
            .collect();
        let named: Vec<String> = others
            .iter()
            .take(LISTED)
            .map(|path| format!("`{path}`"))
            .collect();
        page.push_str("Other declaration files (each at `.../api/` and its path): ");
        page.push_str(&named.join(", "));
        if others.len() > LISTED {
            write!(page, ", and {} more", others.len() - LISTED)
                .expect("writing to a String never fails");
        }
        page.push_str("\n\n");
    }
    page.push_str("```ts\n");
    page.push_str(text.trim_end());
    page.push_str("\n```\n");
    Some(cut(&page, PAGE_LIMIT))
}

/// `DefinitelyTyped`'s package for `name`: `@types/node` for `node`,
/// `@types/babel__core` for `@babel/core`.
#[must_use]
pub fn definitely_typed(name: &str) -> String {
    match name
        .strip_prefix('@')
        .and_then(|scoped| scoped.split_once('/'))
    {
        Some((scope, rest)) => format!("@types/{scope}__{rest}"),
        None => format!("@types/{name}"),
    }
}

#[cfg(test)]
mod tests;
