//! A gem's API from its Ruby source, read the way `RDoc` reads it.
//!
//! The classes and modules under `lib/` (and a Rails engine's `app/`),
//! their methods, attributes and constants, each with the comment block
//! right above it. Parsed with Prism, so the host needs no Ruby.
//!
//! `# :nodoc:` hides a definition (`:nodoc: all`, a namespace and all it
//! holds), `# :stopdoc:` … `# :startdoc:` the definitions between, `#--` …
//! `#++` a part of a comment. Private methods are left out; `initialize` is
//! the class's `new`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use ruby_prism::{
    CallNode, CommentType, ConstantWriteNode, DefNode, FullName as _, Location, Node, ParseResult,
};

use super::archive::read_text;
use super::{PAGE_LIMIT, cut};

/// The most read from all of a gem's Ruby files.
const TOTAL_LIMIT: usize = 32 << 20;
/// Docs shown for the namespace or method a page is about, in characters.
const DOCS_LIMIT: usize = 6_000;
/// Namespaces named on the outline.
const LISTED: usize = 300;
/// Where a gem's API lives: its load path, and a Rails engine's `app/`.
const SOURCE_DIRS: &[&str] = &["lib/", "app/"];
/// Methods Ruby makes private whatever the code says.
const ALWAYS_PRIVATE: &[&str] = &[
    "initialize_copy",
    "initialize_clone",
    "initialize_dup",
    "respond_to_missing?",
];

/// The Ruby files of a `.gem` (in its `data.tar.gz`) under `lib/` and
/// `app/`, by path.
pub fn sources(gem: &[u8]) -> Result<BTreeMap<String, String>> {
    let mut outer = tar::Archive::new(gem);
    for entry in outer.entries().context("reading the .gem")? {
        let entry = entry.context("reading the .gem")?;
        if entry.path().context("reading the .gem")?.as_os_str() != "data.tar.gz" {
            continue;
        }
        let mut data = tar::Archive::new(GzDecoder::new(entry));
        let mut files = BTreeMap::new();
        let mut total = 0;
        for file in data.entries().context("reading the gem's files")? {
            let file = file.context("reading the gem's files")?;
            if !file.header().entry_type().is_file() {
                continue;
            }
            let path = file.path().context("reading the gem's files")?;
            let Some(path) = path
                .to_str()
                .map(|path| path.trim_start_matches("./").to_owned())
                .filter(|path| {
                    std::path::Path::new(path)
                        .extension()
                        .is_some_and(|extension| extension == "rb")
                        && SOURCE_DIRS.iter().any(|dir| path.starts_with(dir))
                })
            else {
                continue;
            };
            if total >= TOTAL_LIMIT {
                break;
            }
            let text = read_text(file)?;
            total += text.len();
            files.insert(path, text);
        }
        return Ok(files);
    }
    Ok(BTreeMap::new())
}

/// A gem's classes and modules, by full name.
#[derive(Debug, Default)]
pub struct Api {
    namespaces: BTreeMap<String, Namespace>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Class,
    Module,
}

/// A class or module, across every file that opens it.
#[derive(Debug)]
struct Namespace {
    kind: Kind,
    superclass: Option<String>,
    docs: String,
    files: BTreeSet<String>,
    /// `include`, `extend` and `prepend`, with what they mix in.
    mixins: Vec<(&'static str, String)>,
    constants: Vec<Constant>,
    attributes: Vec<Attribute>,
    methods: Vec<Method>,
}

#[derive(Debug)]
struct Constant {
    name: String,
    /// The value as written, when it is short and on one line.
    value: Option<String>,
    docs: String,
}

#[derive(Debug)]
struct Attribute {
    name: String,
    access: &'static str,
    /// The namespace's own (`class << self; attr_accessor :x`).
    singleton: bool,
    docs: String,
}

#[derive(Debug)]
struct Method {
    name: String,
    /// The namespace's own (`def self.x`), not its instances'.
    singleton: bool,
    /// The parameters as written; empty without any.
    params: String,
    protected: bool,
    docs: String,
    file: String,
    line: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visibility {
    Public,
    Protected,
    Private,
}

/// What the comments say of a definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Doc {
    Shown,
    /// `:nodoc:`: hidden, but a namespace's inner ones are not.
    Hidden,
    /// `:nodoc: all`, or between `:stopdoc:` and `:startdoc:`.
    HiddenAll,
}

impl Api {
    /// Every class and module `sources` define, a namespace reopened in
    /// several files being one.
    #[must_use]
    pub fn parse(sources: &BTreeMap<String, String>) -> Self {
        let mut api = Self::default();
        for (file, text) in sources {
            let result = ruby_prism::parse(text.as_bytes());
            let mut reader = Reader {
                api: &mut api,
                file,
                notes: Notes::new(&result, text.as_bytes()),
            };
            if let Some(program) = result.node().as_program_node() {
                reader.statements(&program.statements().body(), &mut Scope::default());
            }
        }
        api
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.namespaces.is_empty()
    }

    fn namespace(&mut self, scope: &Scope) -> Option<&mut Namespace> {
        if scope.hidden {
            return None;
        }
        self.namespaces.get_mut(&scope.name)
    }
}

/// Where a definition sits.
#[derive(Debug, Clone)]
struct Scope {
    /// The namespace's full name; empty at the top level.
    name: String,
    /// Under `class << self` or `class_methods do`: methods are the
    /// namespace's own.
    singleton: bool,
    /// What a bare `private`, `protected` or `public` said last.
    visibility: Visibility,
    /// After a bare `module_function`: methods are the module's own.
    module_function: bool,
    /// In a `:nodoc:` namespace: its members are hidden.
    hidden: bool,
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            name: String::new(),
            singleton: false,
            visibility: Visibility::Public,
            module_function: false,
            hidden: true,
        }
    }
}

/// A file's comments by line.
#[derive(Debug, Default)]
struct Notes {
    /// Comments alone on their line, without `#` and one space.
    own: BTreeMap<i32, String>,
    /// Comments after code (`def x # :nodoc:`).
    trailing: BTreeMap<i32, String>,
    /// `:stopdoc:` (true) and `:startdoc:` (false).
    stops: BTreeMap<i32, bool>,
}

impl Notes {
    fn new(result: &ParseResult<'_>, source: &[u8]) -> Self {
        let mut notes = Self::default();
        for comment in result.comments() {
            if comment.type_() != CommentType::InlineComment {
                continue;
            }
            let location = comment.location();
            let line = location.start_line();
            let raw = String::from_utf8_lossy(comment.text());
            let text = raw.strip_prefix('#').unwrap_or(&raw);
            let text = text.strip_prefix(' ').unwrap_or(text).trim_end();
            match text.trim() {
                ":stopdoc:" => {
                    notes.stops.insert(line, true);
                    continue;
                }
                ":startdoc:" => {
                    notes.stops.insert(line, false);
                    continue;
                }
                _ => {}
            }
            let before = &source[..location.start() as usize];
            let alone = before
                .iter()
                .rev()
                .take_while(|&&byte| byte != b'\n')
                .all(u8::is_ascii_whitespace);
            if alone {
                notes.own.insert(line, text.to_owned());
            } else {
                notes.trailing.insert(line, text.to_owned());
            }
        }
        notes
    }

    /// The comment block ending right above `line`, and whether the
    /// definition starting there is shown.
    fn docs(&self, line: i32) -> (String, Doc) {
        let mut block = Vec::new();
        let mut at = line - 1;
        while let Some(text) = self.own.get(&at) {
            block.push(text.as_str());
            at -= 1;
        }
        block.reverse();
        let nodoc = block
            .iter()
            .chain(self.trailing.get(&line).map(String::as_str).as_ref())
            .find_map(|text| text.split_once(":nodoc:").map(|(_, rest)| rest.trim()));
        let doc = match nodoc {
            _ if self.stopped(line) => Doc::HiddenAll,
            Some(rest) if rest.starts_with("all") => Doc::HiddenAll,
            Some(_) => Doc::Hidden,
            None => Doc::Shown,
        };
        (tidy(&block), doc)
    }

    fn stopped(&self, line: i32) -> bool {
        self.stops
            .range(..line)
            .next_back()
            .is_some_and(|(_, &stop)| stop)
    }
}

/// A comment block as docs: without what is between `--` and `++`, magic
/// comments and linter directives, the indent all its lines share, nor
/// blank lines at either end.
fn tidy(block: &[&str]) -> String {
    let mut lines = Vec::new();
    let mut skipping = false;
    for &line in block {
        match line.trim() {
            "--" => skipping = true,
            "++" => skipping = false,
            _ if skipping => {}
            text if is_directive(text) => {}
            _ => lines.push(line),
        }
    }
    let indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let lines: Vec<&str> = lines
        .iter()
        .map(|line| line.get(indent..).unwrap_or_else(|| line.trim_start()))
        .collect();
    let first = lines.iter().position(|line| !line.trim().is_empty());
    let last = lines.iter().rposition(|line| !line.trim().is_empty());
    match (first, last) {
        (Some(first), Some(last)) => lines[first..=last].join("\n"),
        _ => String::new(),
    }
}

/// `frozen_string_literal: true`, `rubocop:disable …` and the like.
fn is_directive(text: &str) -> bool {
    const MAGIC: &[&str] = &[
        "frozen_string_literal:",
        "encoding:",
        "coding:",
        "warn_indent:",
        "shareable_constant_value:",
        "rubocop:",
        "-*-",
    ];
    let lower = text.to_ascii_lowercase();
    MAGIC.iter().any(|magic| lower.starts_with(magic))
}

struct Reader<'a> {
    api: &'a mut Api,
    file: &'a str,
    notes: Notes,
}

impl Reader<'_> {
    fn statements(&mut self, statements: &ruby_prism::NodeList<'_>, scope: &mut Scope) {
        for node in statements {
            self.statement(&node, scope);
        }
    }

    /// A class, module or block body: statements, or one wrapped in
    /// `begin`/`rescue`.
    fn body(&mut self, body: Option<Node<'_>>, scope: &mut Scope) {
        let Some(body) = body else { return };
        if let Some(statements) = body.as_statements_node() {
            self.statements(&statements.body(), scope);
        } else if let Some(begin) = body.as_begin_node() {
            if let Some(statements) = begin.statements() {
                self.statements(&statements.body(), scope);
            }
        } else {
            self.statement(&body, scope);
        }
    }

    fn statement(&mut self, node: &Node<'_>, scope: &mut Scope) {
        if let Some(class) = node.as_class_node() {
            let superclass = class
                .superclass()
                .map(|node| squeeze(&text(&node.location())));
            self.namespace(
                Kind::Class,
                &class.constant_path(),
                superclass,
                class.location().start_line(),
                class.body(),
                scope,
            );
        } else if let Some(module) = node.as_module_node() {
            self.namespace(
                Kind::Module,
                &module.constant_path(),
                None,
                module.location().start_line(),
                module.body(),
                scope,
            );
        } else if let Some(singleton) = node.as_singleton_class_node() {
            if singleton.expression().as_self_node().is_some() {
                self.body(singleton.body(), &mut scope.singleton());
            }
        } else if let Some(def) = node.as_def_node() {
            self.method(&def, scope, scope.visibility);
        } else if let Some(call) = node.as_call_node() {
            self.call(&call, scope);
        } else if let Some(write) = node.as_constant_write_node() {
            self.constant(&write, scope);
        }
    }

    fn namespace(
        &mut self,
        kind: Kind,
        path: &Node<'_>,
        superclass: Option<String>,
        line: i32,
        body: Option<Node<'_>>,
        scope: &Scope,
    ) {
        let name = qualify(&scope.name, &constant_name(path));
        let (docs, doc) = self.notes.docs(line);
        if doc == Doc::HiddenAll {
            return;
        }
        let hidden = doc == Doc::Hidden;
        if !hidden {
            let namespace = self
                .api
                .namespaces
                .entry(name.clone())
                .or_insert_with(|| Namespace {
                    kind,
                    superclass: None,
                    docs: String::new(),
                    files: BTreeSet::new(),
                    mixins: Vec::new(),
                    constants: Vec::new(),
                    attributes: Vec::new(),
                    methods: Vec::new(),
                });
            // `module A::B` reopens B; only `class B` says what B is.
            if kind == Kind::Class {
                namespace.kind = kind;
            }
            if namespace.superclass.is_none() {
                namespace.superclass = superclass;
            }
            if namespace.docs.is_empty() {
                namespace.docs = docs;
            }
            namespace.files.insert(self.file.to_owned());
        }
        let mut inner = Scope {
            name,
            singleton: false,
            visibility: Visibility::Public,
            module_function: false,
            hidden,
        };
        self.body(body, &mut inner);
    }

    fn method(&mut self, def: &DefNode<'_>, scope: &Scope, visibility: Visibility) {
        let line = def.location().start_line();
        let (docs, doc) = self.notes.docs(line);
        let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        let receiver = def.receiver().is_some();
        let singleton = receiver || scope.singleton || scope.module_function;
        // `private` speaks of instance methods: `def self.x` stays public, and
        // `initialize` is the class's `new`.
        let (name, singleton, visibility) = if !singleton && name == "initialize" {
            ("new".to_owned(), true, Visibility::Public)
        } else if receiver {
            (name, singleton, Visibility::Public)
        } else {
            (name, singleton, visibility)
        };
        if doc != Doc::Shown
            || visibility == Visibility::Private
            || (!singleton && ALWAYS_PRIVATE.contains(&name.as_str()))
        {
            return;
        }
        let params = def
            .parameters()
            .map(|params| squeeze(&text(&params.location())))
            .unwrap_or_default();
        let file = self.file.to_owned();
        let Some(namespace) = self.api.namespace(scope) else {
            return;
        };
        let method = Method {
            name,
            singleton,
            params,
            protected: visibility == Visibility::Protected,
            docs,
            file,
            line,
        };
        // Reopened or redefined: the documented definition wins.
        match namespace
            .methods
            .iter_mut()
            .find(|seen| seen.name == method.name && seen.singleton == method.singleton)
        {
            Some(seen) if seen.docs.is_empty() => *seen = method,
            Some(_) => {}
            None => namespace.methods.push(method),
        }
    }

    /// Calls that shape an API: visibility, `module_function`,
    /// attributes, mixins, `private_constant`, `class_methods do`.
    fn call(&mut self, call: &CallNode<'_>, scope: &mut Scope) {
        if call.receiver().is_some() {
            return;
        }
        let arguments: Vec<Node<'_>> = call
            .arguments()
            .map(|arguments| arguments.arguments().iter().collect())
            .unwrap_or_default();
        match call.name().as_slice() {
            verb @ (b"private" | b"protected" | b"public") => {
                let visibility = match verb {
                    b"private" => Visibility::Private,
                    b"protected" => Visibility::Protected,
                    _ => Visibility::Public,
                };
                if arguments.is_empty() {
                    scope.visibility = visibility;
                }
                for argument in &arguments {
                    if let Some(def) = argument.as_def_node() {
                        self.method(&def, scope, visibility);
                    } else if let Some(inner) = argument.as_call_node() {
                        self.attributes(&inner, scope, visibility);
                    } else if let Some(name) = symbol(argument) {
                        self.restrict(scope, &name, scope.singleton, visibility);
                    }
                }
            }
            b"private_class_method" => {
                for name in arguments.iter().filter_map(symbol) {
                    self.restrict(scope, &name, true, Visibility::Private);
                }
            }
            b"module_function" => {
                if arguments.is_empty() {
                    scope.module_function = true;
                }
                let module = Scope {
                    module_function: true,
                    ..scope.clone()
                };
                for argument in &arguments {
                    if let Some(def) = argument.as_def_node() {
                        self.method(&def, &module, Visibility::Public);
                    } else if let Some(name) = symbol(argument)
                        && let Some(namespace) = self.api.namespace(scope)
                    {
                        for method in &mut namespace.methods {
                            if method.name == name && !method.singleton {
                                method.singleton = true;
                            }
                        }
                    }
                }
            }
            b"private_constant" => {
                if let Some(namespace) = self.api.namespace(scope) {
                    let hidden: Vec<String> = arguments.iter().filter_map(symbol).collect();
                    namespace
                        .constants
                        .retain(|constant| !hidden.contains(&constant.name));
                }
            }
            b"attr_reader" | b"attr_writer" | b"attr_accessor" | b"attr" => {
                self.attributes(call, scope, scope.visibility);
            }
            verb @ (b"include" | b"extend" | b"prepend") => {
                let verb = match verb {
                    b"include" => "include",
                    b"extend" => "extend",
                    _ => "prepend",
                };
                if let Some(namespace) = self.api.namespace(scope) {
                    for argument in &arguments {
                        let mixin = squeeze(&text(&argument.location()));
                        namespace.mixins.push((verb, cut(&mixin, 80)));
                    }
                }
            }
            // ActiveSupport::Concern: the including class's own methods.
            b"class_methods" => {
                if let Some(block) = call.block().and_then(|block| block.as_block_node()) {
                    self.body(block.body(), &mut scope.singleton());
                }
            }
            _ => {}
        }
    }

    /// `private :x` and the like, after `x` was defined.
    fn restrict(&mut self, scope: &Scope, name: &str, singleton: bool, visibility: Visibility) {
        let Some(namespace) = self.api.namespace(scope) else {
            return;
        };
        let named = |method: &Method| method.name == name && method.singleton == singleton;
        match visibility {
            Visibility::Private => namespace.methods.retain(|method| !named(method)),
            Visibility::Protected | Visibility::Public => {
                for method in namespace.methods.iter_mut().filter(|method| named(method)) {
                    method.protected = visibility == Visibility::Protected;
                }
            }
        }
    }

    fn attributes(&mut self, call: &CallNode<'_>, scope: &Scope, visibility: Visibility) {
        let access = match call.name().as_slice() {
            b"attr_reader" | b"attr" => "read-only",
            b"attr_writer" => "write-only",
            b"attr_accessor" => "read-write",
            _ => return,
        };
        let (docs, doc) = self.notes.docs(call.location().start_line());
        if call.receiver().is_some() || doc != Doc::Shown || visibility == Visibility::Private {
            return;
        }
        let Some(namespace) = self.api.namespace(scope) else {
            return;
        };
        let names = call
            .arguments()
            .into_iter()
            .flat_map(|arguments| arguments.arguments().iter())
            .filter_map(|argument| symbol(&argument));
        for name in names {
            namespace.attributes.push(Attribute {
                name,
                access,
                singleton: scope.singleton,
                docs: docs.clone(),
            });
        }
    }

    fn constant(&mut self, write: &ConstantWriteNode<'_>, scope: &Scope) {
        let (docs, doc) = self.notes.docs(write.location().start_line());
        if doc != Doc::Shown {
            return;
        }
        let Some(namespace) = self.api.namespace(scope) else {
            return;
        };
        let value = text(&write.value().location());
        namespace.constants.push(Constant {
            name: String::from_utf8_lossy(write.name().as_slice()).into_owned(),
            value: (!value.contains('\n') && value.chars().count() <= 80).then_some(value),
            docs,
        });
    }
}

impl Scope {
    /// The same namespace, for the methods of `class << self`.
    fn singleton(&self) -> Self {
        Self {
            singleton: true,
            visibility: Visibility::Public,
            module_function: false,
            ..self.clone()
        }
    }
}

/// `A::B` for a constant or a constant path, else the source as written.
fn constant_name(node: &Node<'_>) -> String {
    let name = if let Some(path) = node.as_constant_path_node() {
        path.full_name()
    } else if let Some(read) = node.as_constant_read_node() {
        read.full_name()
    } else {
        return text(&node.location());
    };
    name.map_or_else(
        |_| text(&node.location()),
        |name| String::from_utf8_lossy(&name).into_owned(),
    )
}

/// `name` opened inside `outer`: `::Name` is a top-level one.
fn qualify(outer: &str, name: &str) -> String {
    if let Some(absolute) = name.strip_prefix("::") {
        absolute.to_owned()
    } else if outer.is_empty() {
        name.to_owned()
    } else {
        format!("{outer}::{name}")
    }
}

/// `:name` or `"name"`.
fn symbol(node: &Node<'_>) -> Option<String> {
    let bytes = if let Some(symbol) = node.as_symbol_node() {
        symbol.unescaped().to_vec()
    } else {
        node.as_string_node()?.unescaped().to_vec()
    };
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn text(location: &Location<'_>) -> String {
    String::from_utf8_lossy(location.as_slice()).into_owned()
}

/// Whitespace runs, newlines included, as one space.
fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The page for `path` (`StateMachines::Machine`, a method in it as
/// `…#fire` or, the class's own, `….draw`), or the gem's outline; `None`
/// when the gem defines no such thing. `label` names the gem's version.
#[must_use]
pub fn render(api: &Api, path: Option<&str>, label: &str) -> Option<String> {
    let page = match path
        .map(|path| path.trim_start_matches("::"))
        .filter(|path| !path.is_empty())
    {
        None => outline(api, label),
        Some(path) => {
            if let Some(namespace) = api.namespaces.get(path) {
                namespace_page(api, path, namespace, label)
            } else {
                let (owner, name, singleton) = split_method(path)?;
                let method = api
                    .namespaces
                    .get(owner)?
                    .methods
                    .iter()
                    .find(|method| method.name == name && method.singleton == singleton)?;
                method_page(owner, method, label)
            }
        }
    };
    Some(cut(&page, PAGE_LIMIT))
}

/// `A::B#name` (an instance method), `A::B.name` or `A::B::name` (the
/// namespace's own).
fn split_method(path: &str) -> Option<(&str, &str, bool)> {
    if let Some((owner, name)) = path.rsplit_once('#') {
        return Some((owner, name, false));
    }
    if let Some((owner, name)) = path.rsplit_once('.') {
        return Some((owner, name, true));
    }
    let (owner, name) = path.rsplit_once("::")?;
    (!name.starts_with(|c: char| c.is_ascii_uppercase())).then_some((owner, name, true))
}

fn outline(api: &Api, label: &str) -> String {
    let mut page = format!("# {label}: Ruby API\n\n");
    let example = api
        .namespaces
        .iter()
        .find(|(_, namespace)| namespace.kind == Kind::Class)
        .or_else(|| api.namespaces.iter().next())
        .map_or("Name", |(name, _)| name.as_str());
    writeln!(
        page,
        "From the gem's Ruby source, with the comment above each definition as \
         RDoc reads it. Each class or module is at `.../api/` and its name \
         (`.../api/{example}`); one method at `.../api/{example}#name`, or \
         `.../api/{example}.name` for the class's own (`#` is `%23` in a URL).\n"
    )
    .expect("writing to a String never fails");
    page.push_str("## Classes and modules\n\n");
    for (name, namespace) in api.namespaces.iter().take(LISTED) {
        page.push_str(&described(&declaration(name, namespace), &namespace.docs));
        page.push('\n');
    }
    if api.namespaces.len() > LISTED {
        writeln!(page, "- and {} more", api.namespaces.len() - LISTED)
            .expect("writing to a String never fails");
    }
    page
}

/// A namespace with its members, each method's docs in full; with only
/// their summaries when that is too long for one page.
fn namespace_page(api: &Api, name: &str, namespace: &Namespace, label: &str) -> String {
    let full = namespace_text(api, name, namespace, label, true);
    if full.chars().count() <= PAGE_LIMIT {
        return full;
    }
    namespace_text(api, name, namespace, label, false)
}

fn namespace_text(api: &Api, name: &str, namespace: &Namespace, label: &str, full: bool) -> String {
    let mut page = format!("# {}\n\n", declaration(name, namespace));
    let files: Vec<String> = namespace
        .files
        .iter()
        .take(5)
        .map(|file| format!("`{file}`"))
        .collect();
    write!(page, "In {label}, defined in {}", files.join(", "))
        .expect("writing to a String never fails");
    if namespace.files.len() > 5 {
        write!(page, " and {} more", namespace.files.len() - 5)
            .expect("writing to a String never fails");
    }
    page.push_str(".\n\n");
    for verb in ["include", "extend", "prepend"] {
        let mixed: Vec<String> = namespace
            .mixins
            .iter()
            .filter(|(seen, _)| *seen == verb)
            .map(|(_, mixin)| format!("`{mixin}`"))
            .collect();
        if !mixed.is_empty() {
            writeln!(page, "{verb}s {}", mixed.join(", "))
                .expect("writing to a String never fails");
        }
    }
    if !namespace.mixins.is_empty() {
        page.push('\n');
    }
    if !namespace.docs.is_empty() {
        page.push_str(&cut(&namespace.docs, DOCS_LIMIT));
        page.push_str("\n\n");
    }

    let prefix = format!("{name}::");
    let inner: Vec<String> = api
        .namespaces
        .range(prefix.clone()..)
        .take_while(|(inner, _)| inner.starts_with(&prefix))
        .filter(|(inner, _)| !inner[prefix.len()..].contains("::"))
        .map(|(inner, namespace)| described(&declaration(inner, namespace), &namespace.docs))
        .collect();
    section(&mut page, "Classes and modules", &inner);

    let constants: Vec<String> = namespace
        .constants
        .iter()
        .map(|constant| {
            let head = match &constant.value {
                Some(value) => format!("`{} = {value}`", constant.name),
                None => format!("`{}`", constant.name),
            };
            described(&head, &constant.docs)
        })
        .collect();
    section(&mut page, "Constants", &constants);

    let short = name.rsplit("::").next().unwrap_or(name);
    let attributes: Vec<String> = namespace
        .attributes
        .iter()
        .map(|attribute| {
            let head = if attribute.singleton {
                format!("`{short}.{}` ({})", attribute.name, attribute.access)
            } else {
                format!("`{}` ({})", attribute.name, attribute.access)
            };
            described(&head, &attribute.docs)
        })
        .collect();
    section(&mut page, "Attributes", &attributes);

    for (title, singleton) in [("Class methods", true), ("Instance methods", false)] {
        let methods: Vec<&Method> = namespace
            .methods
            .iter()
            .filter(|method| method.singleton == singleton)
            .collect();
        if methods.is_empty() {
            continue;
        }
        writeln!(page, "## {title}\n").expect("writing to a String never fails");
        if !full {
            writeln!(
                page,
                "Too many to show each one's docs here: they are at `.../api/{}`.\n",
                signature_path(name, methods[0])
            )
            .expect("writing to a String never fails");
        }
        for method in methods {
            let head = format!("`{}`{}", signature(short, method), protected(method));
            if full {
                writeln!(page, "### {head}\n").expect("writing to a String never fails");
                if !method.docs.is_empty() {
                    writeln!(page, "{}\n", cut(&method.docs, DOCS_LIMIT))
                        .expect("writing to a String never fails");
                }
            } else {
                writeln!(page, "{}", described(&head, &method.docs))
                    .expect("writing to a String never fails");
            }
        }
        if !full {
            page.push('\n');
        }
    }
    page
}

fn method_page(owner: &str, method: &Method, label: &str) -> String {
    let short = owner.rsplit("::").next().unwrap_or(owner);
    let mut page = format!(
        "# `{}`{}\n\nIn {label}: `{owner}`, `{}` line {}.\n\n",
        signature(short, method),
        protected(method),
        method.file,
        method.line
    );
    if !method.docs.is_empty() {
        page.push_str(&cut(&method.docs, DOCS_LIMIT));
        page.push('\n');
    }
    page
}

/// `class StateMachines::Machine < Base`, `module StateMachines`.
fn declaration(name: &str, namespace: &Namespace) -> String {
    match (namespace.kind, &namespace.superclass) {
        (Kind::Class, Some(superclass)) => format!("class `{name}` < `{superclass}`"),
        (Kind::Class, None) => format!("class `{name}`"),
        (Kind::Module, _) => format!("module `{name}`"),
    }
}

/// `Machine.new(owner_class, *args)`, `Machine#fire(event)`, `Machine#name`.
fn signature(short: &str, method: &Method) -> String {
    let joint = if method.singleton { '.' } else { '#' };
    if method.params.is_empty() {
        format!("{short}{joint}{}", method.name)
    } else {
        format!("{short}{joint}{}({})", method.name, method.params)
    }
}

/// Where a method's own page is: `StateMachines::Machine#fire`.
fn signature_path(owner: &str, method: &Method) -> String {
    let joint = if method.singleton { '.' } else { '#' };
    format!("{owner}{joint}{}", method.name)
}

const fn protected(method: &Method) -> &'static str {
    if method.protected { " (protected)" } else { "" }
}

fn section(page: &mut String, title: &str, lines: &[String]) {
    if !lines.is_empty() {
        writeln!(page, "## {title}\n\n{}\n", lines.join("\n"))
            .expect("writing to a String never fails");
    }
}

/// `- head: the docs' first line`.
fn described(head: &str, docs: &str) -> String {
    match docs.lines().map(str::trim).find(|line| !line.is_empty()) {
        Some(summary) => format!("- {head}: {}", cut(summary, 200)),
        None => format!("- {head}"),
    }
}

#[cfg(test)]
mod tests;
