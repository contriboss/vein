//! A crate's API from docs.rs's rustdoc JSON, as markdown: the crate's
//! outline, or one module or item. Read as loose JSON, so a change of
//! rustdoc's format version degrades a signature, not the page.

use std::fmt::Write as _;
use std::io::Read;

use anyhow::{Context, Result};
use serde_json::Value;

use super::{PAGE_LIMIT, cut};

/// The most JSON one crate may decompress to (the largest run to tens of MB).
const JSON_LIMIT: u64 = 256 << 20;
/// Docs shown for the crate or item a page is about, in characters.
const DOCS_LIMIT: usize = 6_000;

/// docs.rs's zstd-compressed rustdoc JSON, parsed.
pub fn decode(zst: &[u8]) -> Result<Value> {
    let decoder =
        ruzstd::decoding::StreamingDecoder::new(zst).context("reading docs.rs's zstd stream")?;
    let mut json = Vec::new();
    decoder
        .take(JSON_LIMIT)
        .read_to_end(&mut json)
        .context("decompressing docs.rs's rustdoc JSON")?;
    serde_json::from_slice(&json).context("parsing docs.rs's rustdoc JSON")
}

/// The page for `path` (`runtime::bounds`, with or without the crate's
/// name), or the crate's outline; `None` when the crate has no such public
/// path.
#[must_use]
pub fn render(doc: &Value, path: Option<&str>) -> Option<String> {
    let krate = Crate::new(doc)?;
    let page = match path.filter(|path| !path.is_empty()) {
        None => krate.outline(),
        Some(path) => {
            let (id, item) = krate.find(path)?;
            krate.item_page(id, item)
        }
    };
    Some(cut(&page, PAGE_LIMIT))
}

struct Crate<'a> {
    doc: &'a Value,
    name: String,
    version: String,
}

impl<'a> Crate<'a> {
    fn new(doc: &'a Value) -> Option<Self> {
        let root = item(doc, doc.get("root")?)?;
        Some(Self {
            doc,
            name: root.get("name")?.as_str()?.to_owned(),
            version: doc
                .get("crate_version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        })
    }

    fn item(&self, id: &Value) -> Option<&'a Value> {
        item(self.doc, id)
    }

    /// `state_machines::runtime::Machine` for item `id`, when it is public.
    fn path_of(&self, id: &Value) -> Option<String> {
        let key = id_key(id)?;
        let summary = self.doc.get("paths")?.get(&key)?;
        let parts: Vec<&str> = summary
            .get("path")?
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        Some(parts.join("::"))
    }

    /// The local item at public path `path`.
    fn find(&self, path: &str) -> Option<(String, &'a Value)> {
        let wanted = path.trim_start_matches("crate::");
        let full = if wanted == self.name || wanted.starts_with(&format!("{}::", self.name)) {
            wanted.to_owned()
        } else {
            format!("{}::{wanted}", self.name)
        };
        let paths = self.doc.get("paths")?.as_object()?;
        let (id, _) = paths.iter().find(|(_, summary)| {
            summary.get("crate_id").and_then(Value::as_u64) == Some(0)
                && summary
                    .get("path")
                    .and_then(Value::as_array)
                    .is_some_and(|parts| {
                        parts
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("::")
                            == full
                    })
        })?;
        let item = self.doc.get("index")?.get(id)?;
        Some((id.clone(), item))
    }

    fn outline(&self) -> String {
        let mut page = format!("# {} {}\n\n", self.name, self.version);
        let root = self.doc.get("root").and_then(|id| self.item(id));
        if let Some(docs) = root.and_then(docs_of) {
            page.push_str(&cut(docs, DOCS_LIMIT));
            page.push_str("\n\n");
        }
        if let Some(module) = root.and_then(|root| root.pointer("/inner/module")) {
            self.list_module(&mut page, module);
        }
        page
    }

    fn item_page(&self, id: String, item: &'a Value) -> String {
        let path = self.path_of(&Value::String(id)).unwrap_or_default();
        let (kind, inner) = kind_of(item);
        let mut page = format!("# {} `{path}`\n\n", kind_label(kind));
        if let Some(signature) = Self::signature(item) {
            writeln!(page, "```rust\n{signature}\n```\n").expect("writing to a String never fails");
        }
        if let Some(docs) = docs_of(item) {
            page.push_str(&cut(docs, DOCS_LIMIT));
            page.push_str("\n\n");
        }
        match kind {
            "module" => self.list_module(&mut page, inner),
            "struct" | "enum" | "union" => {
                self.list_members(&mut page, inner);
                self.list_methods(&mut page, inner);
            }
            "trait" => self.list_trait_items(&mut page, inner),
            _ => {}
        }
        page
    }

    /// A module's public items by kind, re-exports included.
    fn list_module(&self, page: &mut String, module: &Value) {
        let mut groups: Vec<(&str, Vec<String>)> = Vec::new();
        for id in module
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            // Re-exported from another crate: its items are not in this index.
            if let Some(source) = self.foreign_use(id) {
                let line = format!("- `pub use {source}`");
                match groups.iter_mut().find(|(group, _)| *group == "use") {
                    Some((_, lines)) => lines.push(line),
                    None => groups.push(("use", vec![line])),
                }
                continue;
            }
            for (name, target) in self.exported(id) {
                let (kind, _) = kind_of(target);
                if matches!(kind, "impl" | "use" | "extern_crate") {
                    continue;
                }
                let line = match first_line(target) {
                    Some(summary) => format!("- `{name}`: {summary}"),
                    None => format!("- `{name}`"),
                };
                match groups.iter_mut().find(|(group, _)| *group == kind) {
                    Some((_, lines)) => lines.push(line),
                    None => groups.push((kind, vec![line])),
                }
            }
        }
        groups.sort_by_key(|(kind, _)| kind_order(kind));
        for (kind, mut lines) in groups {
            lines.sort();
            lines.dedup();
            writeln!(page, "## {}\n\n{}\n", kind_plural(kind), lines.join("\n"))
                .expect("writing to a String never fails");
        }
    }

    /// `source` (with `::*` for a glob) of a `use` whose target is in
    /// another crate.
    fn foreign_use(&self, id: &Value) -> Option<String> {
        let import = self.item(id)?.pointer("/inner/use")?;
        if import.get("id").and_then(|id| self.item(id)).is_some() {
            return None;
        }
        let source = import.get("source").and_then(Value::as_str)?;
        let glob = import
            .get("is_glob")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Some(if glob {
            format!("{source}::*")
        } else {
            source.to_owned()
        })
    }

    /// What item `id` puts in its module: itself, or what a `use` brings
    /// (a glob, everything public in the module it names).
    fn exported(&self, id: &Value) -> Vec<(String, &'a Value)> {
        let Some(item) = self.item(id) else {
            return Vec::new();
        };
        let Some(import) = item.pointer("/inner/use") else {
            let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
            return vec![(name.to_owned(), item)];
        };
        let target = import.get("id").and_then(|id| self.item(id));
        let is_glob = import
            .get("is_glob")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match (target, is_glob) {
            (Some(module), true) => module
                .pointer("/inner/module/items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|id| {
                    let item = self.item(id)?;
                    Some((item.get("name")?.as_str()?.to_owned(), item))
                })
                .collect(),
            (Some(target), false) => {
                let name = import
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                vec![(name.to_owned(), target)]
            }
            // A re-export from another crate: not in this crate's index.
            (None, _) => Vec::new(),
        }
    }

    fn list_members(&self, page: &mut String, inner: &Value) {
        let mut lines = Vec::new();
        let fields = inner
            .pointer("/kind/plain/fields")
            .or_else(|| inner.get("fields"))
            .or_else(|| inner.pointer("/kind/tuple"));
        for id in fields.and_then(Value::as_array).into_iter().flatten() {
            if let Some(field) = self.item(id)
                && let Some(ty) = field.pointer("/inner/struct_field")
            {
                let name = field.get("name").and_then(Value::as_str).unwrap_or("_");
                lines.push(described(&format!("`{name}: {}`", ty_str(ty)), field));
            }
        }
        for id in inner
            .get("variants")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(variant) = self.item(id) {
                let name = variant
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                lines.push(described(&format!("`{name}`"), variant));
            }
        }
        if !lines.is_empty() {
            let title = if inner.get("variants").is_some() {
                "Variants"
            } else {
                "Fields"
            };
            writeln!(page, "## {title}\n\n{}\n", lines.join("\n"))
                .expect("writing to a String never fails");
        }
    }

    /// Inherent methods, from the type's `impl` blocks without a trait.
    fn list_methods(&self, page: &mut String, inner: &Value) {
        let mut lines = Vec::new();
        let mut traits = Vec::new();
        for id in inner
            .get("impls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(block) = self.item(id).and_then(|item| item.pointer("/inner/impl")) else {
                continue;
            };
            if let Some(implemented) = block.get("trait").filter(|t| !t.is_null()) {
                let name = path_str(implemented);
                // Auto traits are synthetic; `StructuralPartialEq` is the
                // compiler's marker behind `derive(PartialEq)`.
                if !block
                    .get("is_synthetic")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    && block.get("blanket_impl").is_none_or(Value::is_null)
                    && name != "StructuralPartialEq"
                {
                    traits.push(format!("`{name}`"));
                }
                continue;
            }
            for id in block
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(method) = self.item(id)
                    && let Some(signature) = Self::signature(method)
                {
                    lines.push(described(&format!("`{signature}`"), method));
                }
            }
        }
        if !lines.is_empty() {
            writeln!(page, "## Methods\n\n{}\n", lines.join("\n"))
                .expect("writing to a String never fails");
        }
        if !traits.is_empty() {
            traits.sort();
            traits.dedup();
            writeln!(page, "## Implements\n\n{}\n", traits.join(", "))
                .expect("writing to a String never fails");
        }
    }

    fn list_trait_items(&self, page: &mut String, inner: &Value) {
        let mut lines = Vec::new();
        for id in inner
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(member) = self.item(id) {
                let signature = Self::signature(member).unwrap_or_else(|| {
                    member
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                });
                lines.push(described(&format!("`{signature}`"), member));
            }
        }
        if !lines.is_empty() {
            writeln!(page, "## Items\n\n{}\n", lines.join("\n"))
                .expect("writing to a String never fails");
        }
    }

    /// A one-line declaration: `pub fn name<T>(a: T) -> Out`, `pub struct S<T>`.
    fn signature(item: &Value) -> Option<String> {
        let name = item.get("name").and_then(Value::as_str)?;
        let (kind, inner) = kind_of(item);
        let generics = generic_params(inner.get("generics"));
        Some(match kind {
            "function" => {
                let sig = inner.get("sig")?;
                let inputs: Vec<String> = sig
                    .get("inputs")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|input| {
                        let pair = input.as_array()?;
                        let name = pair.first()?.as_str()?;
                        let ty = pair.get(1)?;
                        Some(if name == "self" {
                            self_str(ty)
                        } else {
                            format!("{name}: {}", ty_str(ty))
                        })
                    })
                    .collect();
                let output = sig
                    .get("output")
                    .filter(|output| !output.is_null())
                    .map(|output| format!(" -> {}", ty_str(output)))
                    .unwrap_or_default();
                format!("fn {name}{generics}({}){output}", inputs.join(", "))
            }
            "struct" | "enum" | "union" | "trait" => format!("{kind} {name}{generics}"),
            "type_alias" => {
                let ty = inner.get("type").map_or_else(|| "…".into(), ty_str);
                format!("type {name}{generics} = {ty}")
            }
            "constant" => {
                let ty = inner.get("type").map_or_else(|| "…".into(), ty_str);
                format!("const {name}: {ty}")
            }
            "static" => {
                let ty = inner.get("type").map_or_else(|| "…".into(), ty_str);
                format!("static {name}: {ty}")
            }
            "macro" | "proc_macro" => format!("macro {name}!"),
            "assoc_type" => format!("type {name}"),
            "assoc_const" => {
                let ty = inner.get("type").map_or_else(|| "…".into(), ty_str);
                format!("const {name}: {ty}")
            }
            "module" => format!("mod {name}"),
            _ => return None,
        })
    }
}

fn item<'a>(doc: &'a Value, id: &Value) -> Option<&'a Value> {
    doc.get("index")?.get(id_key(id)?)
}

/// Ids are numbers in format 61, strings in older ones.
fn id_key(id: &Value) -> Option<String> {
    match id {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// The item's kind (`struct`, `function`, …) and its details.
fn kind_of(item: &Value) -> (&str, &Value) {
    item.get("inner")
        .and_then(Value::as_object)
        .and_then(|inner| inner.iter().next())
        .map_or(("unknown", &Value::Null), |(kind, inner)| {
            (kind.as_str(), inner)
        })
}

fn docs_of(item: &Value) -> Option<&str> {
    item.get("docs")
        .and_then(Value::as_str)
        .filter(|docs| !docs.trim().is_empty())
}

/// The first paragraph's first line, as rustdoc's summaries do.
fn first_line(item: &Value) -> Option<String> {
    let line = docs_of(item)?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    Some(cut(line, 200))
}

fn described(head: &str, item: &Value) -> String {
    match first_line(item) {
        Some(summary) => format!("- {head}: {summary}"),
        None => format!("- {head}"),
    }
}

fn generic_params(generics: Option<&Value>) -> String {
    let names: Vec<&str> = generics
        .and_then(|generics| generics.get("params"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|param| {
            // Elided lifetimes and `impl Trait` arguments are not written out.
            !param
                .pointer("/kind/type/is_synthetic")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|param| param.get("name").and_then(Value::as_str))
        .collect();
    if names.is_empty() {
        String::new()
    } else {
        format!("<{}>", names.join(", "))
    }
}

/// `self`, `&self`, `&mut self`, or `self: T`.
fn self_str(ty: &Value) -> String {
    if ty.get("generic").and_then(Value::as_str) == Some("Self") {
        return "self".into();
    }
    if let Some(reference) = ty.get("borrowed_ref")
        && reference.pointer("/type/generic").and_then(Value::as_str) == Some("Self")
    {
        let mutable = reference
            .get("is_mutable")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        return if mutable {
            "&mut self".into()
        } else {
            "&self".into()
        };
    }
    format!("self: {}", ty_str(ty))
}

/// A rustdoc type as Rust spells it; `…` for a form this does not know.
fn ty_str(ty: &Value) -> String {
    let Some((form, value)) = ty.as_object().and_then(|ty| ty.iter().next()) else {
        return "…".into();
    };
    match form.as_str() {
        "resolved_path" => path_str(value),
        "generic" | "primitive" => value.as_str().unwrap_or("…").to_owned(),
        "borrowed_ref" => {
            let lifetime = value
                .get("lifetime")
                .and_then(Value::as_str)
                .map(|l| format!("{l} "))
                .unwrap_or_default();
            let mutable = if value
                .get("is_mutable")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "mut "
            } else {
                ""
            };
            format!(
                "&{lifetime}{mutable}{}",
                value.get("type").map_or_else(|| "…".into(), ty_str)
            )
        }
        "raw_pointer" => {
            let mutable = if value
                .get("is_mutable")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "mut"
            } else {
                "const"
            };
            format!(
                "*{mutable} {}",
                value.get("type").map_or_else(|| "…".into(), ty_str)
            )
        }
        "slice" => format!("[{}]", ty_str(value)),
        "array" => format!(
            "[{}; {}]",
            value.get("type").map_or_else(|| "…".into(), ty_str),
            value.get("len").and_then(Value::as_str).unwrap_or("_")
        ),
        "tuple" => {
            let parts: Vec<String> = value.as_array().into_iter().flatten().map(ty_str).collect();
            format!("({})", parts.join(", "))
        }
        "impl_trait" => format!("impl {}", bounds_str(value)),
        "dyn_trait" => {
            let traits: Vec<String> = value
                .get("traits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|bound| bound.get("trait").map(path_str))
                .collect();
            format!("dyn {}", traits.join(" + "))
        }
        "qualified_path" => {
            let name = value.get("name").and_then(Value::as_str).unwrap_or("…");
            let self_type = value.get("self_type").map_or_else(|| "…".into(), ty_str);
            match value.get("trait").filter(|t| !t.is_null()) {
                Some(trait_path) => format!("<{self_type} as {}>::{name}", path_str(trait_path)),
                None => format!("{self_type}::{name}"),
            }
        }
        "function_pointer" => "fn(…)".into(),
        "infer" => "_".into(),
        _ => "…".into(),
    }
}

/// `Vec<LifecycleRule<Self>>`: a path and its generic arguments.
fn path_str(path: &Value) -> String {
    let name = path.get("path").and_then(Value::as_str).unwrap_or("…");
    let args: Vec<String> = path
        .pointer("/args/angle_bracketed/args")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|arg| {
            match arg
                .as_object()
                .and_then(|arg| arg.iter().next())
                .map(|(form, value)| (form.as_str(), value))
            {
                Some(("type", ty)) => ty_str(ty),
                Some(("lifetime", lifetime)) => lifetime.as_str().unwrap_or("'_").to_owned(),
                Some(("const", _)) => "{const}".into(),
                _ => "_".into(),
            }
        })
        .collect();
    if args.is_empty() {
        name.to_owned()
    } else {
        format!("{name}<{}>", args.join(", "))
    }
}

fn bounds_str(bounds: &Value) -> String {
    let parts: Vec<String> = bounds
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|bound| {
            match bound
                .as_object()
                .and_then(|b| b.iter().next())
                .map(|(form, value)| (form.as_str(), value))
            {
                Some(("trait_bound", value)) => value.get("trait").map(path_str),
                Some(("outlives", lifetime)) => lifetime.as_str().map(str::to_owned),
                _ => None,
            }
        })
        .collect();
    parts.join(" + ")
}

const fn kind_order(kind: &str) -> u8 {
    match kind.as_bytes() {
        b"module" => 0,
        b"macro" | b"proc_macro" => 1,
        b"struct" => 2,
        b"enum" => 3,
        b"union" => 4,
        b"trait" | b"trait_alias" => 5,
        b"function" => 6,
        b"type_alias" => 7,
        b"constant" => 8,
        b"static" => 9,
        b"use" => 10,
        _ => 11,
    }
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "module" => "Module",
        "struct" => "Struct",
        "enum" => "Enum",
        "union" => "Union",
        "trait" | "trait_alias" => "Trait",
        "function" => "Function",
        "type_alias" => "Type",
        "constant" => "Constant",
        "static" => "Static",
        "macro" | "proc_macro" => "Macro",
        other => other,
    }
}

fn kind_plural(kind: &str) -> &str {
    match kind {
        "module" => "Modules",
        "struct" => "Structs",
        "enum" => "Enums",
        "union" => "Unions",
        "trait" | "trait_alias" => "Traits",
        "function" => "Functions",
        "type_alias" => "Types",
        "constant" => "Constants",
        "static" => "Statics",
        "macro" | "proc_macro" => "Macros",
        "use" => "Re-exports",
        other => other,
    }
}

#[cfg(test)]
mod tests;
