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

/// How deep `export * from` chains are followed from the main file.
const REEXPORT_DEPTH: usize = 8;
/// The most exports recorded for one package.
const EXPORT_LIMIT: usize = 5_000;

/// What `typings`' main file exports, following relative `export * from`
/// into the package's other declaration files, for the exports index.
/// Paths are `{package}::{symbol}`: everything is reached from the root.
#[must_use]
pub fn exports(typings: &Typings, package: &str) -> Vec<vein_adapter::PackageExport> {
    let mut walk = ExportWalk {
        typings,
        package,
        seen: std::collections::BTreeSet::new(),
        exports: Vec::new(),
    };
    if let Some(main) = typings.main() {
        walk.file(main, 0);
    }
    walk.exports
}

struct ExportWalk<'a> {
    typings: &'a Typings,
    package: &'a str,
    seen: std::collections::BTreeSet<String>,
    exports: Vec<vein_adapter::PackageExport>,
}

impl ExportWalk<'_> {
    fn file(&mut self, path: &str, depth: usize) {
        if depth > REEXPORT_DEPTH || !self.seen.insert(path.to_owned()) {
            return;
        }
        let Some(source) = self.typings.files.get(path) else {
            return;
        };
        let Ok(tree) = crate::parse::typescript::parse(source.as_bytes()) else {
            return;
        };
        let source = source.as_bytes();
        let root = tree.root_node();
        let mut cursor = root.walk();
        let statements: Vec<_> = root.children(&mut cursor).collect();
        // `declare function a(): void; export { a }`: kinds of local names.
        let local: BTreeMap<String, &'static str> = statements
            .iter()
            .flat_map(|node| declared_names(unwrap_ambient(*node), source))
            .collect();
        for statement in statements
            .iter()
            .filter(|node| node.kind() == "export_statement")
        {
            if self.exports.len() >= EXPORT_LIMIT {
                return;
            }
            self.statement(*statement, source, &local, path, depth);
        }
    }

    fn statement(
        &mut self,
        statement: tree_sitter::Node,
        source: &[u8],
        local: &BTreeMap<String, &'static str>,
        path: &str,
        depth: usize,
    ) {
        let docs = jsdoc_summary(statement, source);
        if let Some(declaration) = statement.child_by_field_name("declaration") {
            let declaration = unwrap_ambient(declaration);
            for (name, kind) in declared_names(declaration, source) {
                self.push(&name, kind, docs.clone());
            }
            return;
        }
        let from = statement
            .child_by_field_name("source")
            .map(|node| text(node, source).trim_matches(['"', '\'']).to_owned());
        let mut cursor = statement.walk();
        for child in statement.children(&mut cursor) {
            match child.kind() {
                "export_clause" => {
                    let mut specifiers = child.walk();
                    for specifier in child
                        .children(&mut specifiers)
                        .filter(|node| node.kind() == "export_specifier")
                    {
                        let Some(name) = specifier.child_by_field_name("name") else {
                            continue;
                        };
                        let name = text(name, source);
                        let exported = specifier
                            .child_by_field_name("alias")
                            .map_or(name, |alias| text(alias, source));
                        let kind = local.get(name).copied().unwrap_or("reexport");
                        self.push(exported.trim_matches(['"', '\'']), kind, docs.clone());
                    }
                    return;
                }
                "namespace_export" => {
                    let mut names = child.walk();
                    if let Some(name) = child
                        .children(&mut names)
                        .find(|n| n.kind() == "identifier")
                    {
                        self.push(text(name, source), "namespace", docs);
                    }
                    return;
                }
                _ => {}
            }
        }
        // `export * from "./x"`: the other file's exports are this one's.
        if let Some(target) = from
            .as_deref()
            .and_then(|spec| resolve(self.typings, path, spec))
        {
            self.file(&target, depth + 1);
        }
    }

    fn push(&mut self, symbol: &str, kind: &str, docs: Option<String>) {
        if symbol.is_empty() || self.exports.len() >= EXPORT_LIMIT {
            return;
        }
        self.exports.push(vein_adapter::PackageExport {
            symbol: symbol.to_owned(),
            kind: kind.to_owned(),
            path: format!("{}::{symbol}", self.package),
            docs,
        });
    }
}

/// `declare function f()` is a `function_signature` inside an
/// `ambient_declaration`; the declaration is what names it.
fn unwrap_ambient(node: tree_sitter::Node) -> tree_sitter::Node {
    if node.kind() != "ambient_declaration" {
        return node;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.is_named() && child.kind() != "comment")
        .unwrap_or(node)
}

/// Every name a declaration introduces (`const a = 1, b = 2` has two).
fn declared_names(node: tree_sitter::Node, source: &[u8]) -> Vec<(String, &'static str)> {
    let kind = match node.kind() {
        "function_declaration" | "function_signature" | "generator_function_declaration" => {
            "function"
        }
        "class_declaration" | "abstract_class_declaration" => "class",
        "interface_declaration" => "interface",
        "type_alias_declaration" => "type",
        "enum_declaration" => "enum",
        "internal_module" | "module" => "namespace",
        "lexical_declaration" | "variable_declaration" => {
            let mut cursor = node.walk();
            return node
                .children(&mut cursor)
                .filter(|child| child.kind() == "variable_declarator")
                .filter_map(|declarator| declarator.child_by_field_name("name"))
                .filter(|name| name.kind() == "identifier")
                .map(|name| (text(name, source).to_owned(), "variable"))
                .collect();
        }
        _ => return Vec::new(),
    };
    node.child_by_field_name("name")
        .map(|name| vec![(text(name, source).to_owned(), kind)])
        .unwrap_or_default()
}

/// The first line of the `/** ... */` right before `node`.
fn jsdoc_summary(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let comment = node
        .prev_named_sibling()
        .filter(|n| n.kind() == "comment")?;
    let raw = text(comment, source);
    if !raw.starts_with("/**") {
        return None;
    }
    raw.trim_start_matches("/**")
        .trim_end_matches("*/")
        .lines()
        .map(|line| line.trim().trim_start_matches('*').trim())
        .find(|line| !line.is_empty() && !line.starts_with('@'))
        .map(|line| line.chars().take(200).collect())
}

fn text<'s>(node: tree_sitter::Node, source: &'s [u8]) -> &'s str {
    node.utf8_text(source).unwrap_or_default()
}

/// The declaration file a relative `from` names, seen from `from_file`:
/// `./x.js` is declared by `x.d.ts`, `./x` by `x.d.ts` or `x/index.d.ts`.
fn resolve(typings: &Typings, from_file: &str, spec: &str) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let dir = from_file.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for part in spec.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    let stem = [".js", ".cjs", ".mjs", ".ts"]
        .iter()
        .find_map(|ext| joined.strip_suffix(ext))
        .unwrap_or(&joined);
    let declared = match joined.rsplit('.').next() {
        Some("cjs") => vec![format!("{stem}.d.cts")],
        Some("mjs") => vec![format!("{stem}.d.mts")],
        _ => Vec::new(),
    };
    declared
        .into_iter()
        .chain([
            joined.clone(),
            format!("{stem}.d.ts"),
            format!("{stem}.d.cts"),
            format!("{stem}.d.mts"),
            format!("{joined}/index.d.ts"),
        ])
        .find(|candidate| typings.files.contains_key(candidate))
}

#[cfg(test)]
mod tests;
