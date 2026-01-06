use anyhow::{Context, Result};
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolType {
    Class,
    Module,
}

impl SymbolType {
    pub fn as_str(&self) -> &str {
        match self {
            SymbolType::Class => "class",
            SymbolType::Module => "module",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RubySymbol {
    pub symbol_type: SymbolType,
    pub name: String,
    pub parent: Option<String>,
    pub line: usize,
}

/// Extract class and module definitions from Ruby source code using tree-sitter
pub fn extract_symbols(source: &str) -> Result<Vec<RubySymbol>> {
    let mut parser = Parser::new();
    let language = tree_sitter_ruby::LANGUAGE.into();
    parser
        .set_language(&language)
        .context("failed to set Ruby language")?;

    let tree = parser
        .parse(source, None)
        .context("failed to parse Ruby source")?;

    let root = tree.root_node();

    // Query for class and module definitions
    // This pattern matches both top-level and nested class/module definitions
    let query_str = r#"
        (class name: (constant) @class_name) @class
        (class name: (scope_resolution scope: (_) @class_scope name: (constant) @class_scoped_name)) @scoped_class
        (module name: (constant) @module_name) @module
        (module name: (scope_resolution scope: (_) @module_scope name: (constant) @module_scoped_name)) @scoped_module
    "#;

    let query = Query::new(&language, query_str).context("failed to create tree-sitter query")?;
    let mut cursor = QueryCursor::new();
    let source_bytes = source.as_bytes();

    let mut symbols = Vec::new();

    let mut matches = cursor.matches(&query, root, source_bytes);
    while let Some(match_) = matches.next() {
        let mut symbol_type: Option<SymbolType> = None;
        let mut name: Option<String> = None;
        let mut parent: Option<String> = None;
        let mut line: usize = 0;

        for capture in match_.captures {
            let capture_name = &query.capture_names()[capture.index as usize];
            let node_text = &source[capture.node.byte_range()];

            match &**capture_name {
                "class" | "scoped_class" => {
                    symbol_type = Some(SymbolType::Class);
                    line = capture.node.start_position().row + 1;
                }
                "module" | "scoped_module" => {
                    symbol_type = Some(SymbolType::Module);
                    line = capture.node.start_position().row + 1;
                }
                "class_name" | "module_name" => {
                    name = Some(node_text.to_string());
                }
                "class_scoped_name" | "module_scoped_name" => {
                    name = Some(node_text.to_string());
                }
                "class_scope" | "module_scope" => {
                    parent = Some(node_text.to_string());
                }
                _ => {}
            }
        }

        if let (Some(typ), Some(n)) = (symbol_type, name) {
            // Handle scope resolution (Foo::Bar::Baz)
            let full_name = if let Some(p) = parent.as_ref() {
                format!("{}::{}", p, n)
            } else {
                n.clone()
            };

            symbols.push(RubySymbol {
                symbol_type: typ,
                name: full_name,
                parent,
                line,
            });
        }
    }

    Ok(symbols)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_class() {
        let source = r#"
class Foo
end
"#;
        let symbols = extract_symbols(source).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].symbol_type, SymbolType::Class);
        assert_eq!(symbols[0].name, "Foo");
        assert_eq!(symbols[0].parent, None);
    }

    #[test]
    fn test_extract_simple_module() {
        let source = r#"
module Bar
end
"#;
        let symbols = extract_symbols(source).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].symbol_type, SymbolType::Module);
        assert_eq!(symbols[0].name, "Bar");
    }

    #[test]
    fn test_extract_nested_class() {
        let source = r#"
module Foo
  class Bar
  end
end
"#;
        let symbols = extract_symbols(source).unwrap();
        assert_eq!(symbols.len(), 2);
        assert!(symbols
            .iter()
            .any(|s| s.name == "Foo" && s.symbol_type == SymbolType::Module));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Bar" && s.symbol_type == SymbolType::Class));
    }

    #[test]
    fn test_extract_scoped_class() {
        let source = r#"
class Foo::Bar::Baz
end
"#;
        let symbols = extract_symbols(source).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].symbol_type, SymbolType::Class);
        assert_eq!(symbols[0].name, "Foo::Bar::Baz");
    }

    #[test]
    fn test_extract_nested_class_parent() {
        // Test that nested classes capture parent relationships
        // Note: Current implementation only handles scope resolution (::), not lexical nesting
        let source = r#"
class Foo::Bar
end
"#;
        let symbols = extract_symbols(source).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Foo::Bar");
        assert_eq!(symbols[0].parent, Some("Foo".to_string()));
    }

    #[test]
    fn test_extract_complex_scope_chain() {
        // Test extraction of a class with multiple levels of scope resolution
        let source = r#"
class A::B::C::D
end
"#;
        let symbols = extract_symbols(source).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].symbol_type, SymbolType::Class);
        assert_eq!(symbols[0].name, "A::B::C::D");
        assert_eq!(symbols[0].parent, Some("A::B::C".to_string()));
    }
}
