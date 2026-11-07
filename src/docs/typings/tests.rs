use super::*;
use crate::test_support::tar_gz;

/// Declarations shaped like a real typed package's: `package.json` points
/// at `dist/`, beside a stray `index.d.ts` and a dependency's.
fn tarball(manifest: &str) -> Vec<u8> {
    tar_gz(&[
        ("package/package.json", manifest.as_bytes()),
        ("package/dist/index.d.ts", b"export declare function leftPad(text: string, length: number, fill?: string): string;\n"),
        ("package/dist/options.d.ts", b"export interface Options { fill: string }\n"),
        ("package/index.d.ts", b"export * from './dist';\n"),
        ("package/node_modules/dep/index.d.ts", b"export {};\n"),
        ("package/index.js", b"module.exports = {};\n"),
    ])
}

#[test]
fn the_declared_file_comes_first_and_the_rest_are_listed() {
    for manifest in [
        r#"{"name":"left-pad","types":"./dist/index.d.ts"}"#,
        r#"{"name":"left-pad","typings":"dist/index.d.ts"}"#,
        r#"{"name":"left-pad","exports":{".":{"types":"./dist/index.d.ts","default":"./index.js"}}}"#,
    ] {
        let typings = read(&tarball(manifest)).unwrap();
        assert_eq!(typings.main(), Some("dist/index.d.ts"), "{manifest}");
        assert!(
            !typings.files.contains_key("node_modules/dep/index.d.ts"),
            "{manifest}"
        );
        let page = render(&typings, None, "left-pad 1.3.0: TypeScript declarations").unwrap();
        assert!(
            page.contains("`dist/index.d.ts`, as `package.json` declares"),
            "{page}"
        );
        assert!(
            page.contains("```ts\nexport declare function leftPad("),
            "{page}"
        );
        assert!(page.contains("`dist/options.d.ts`, `index.d.ts`"), "{page}");
    }
}

#[test]
fn undeclared_packages_start_at_index_d_ts_and_files_can_be_asked_for() {
    let typings = read(&tarball(r#"{"name":"left-pad"}"#)).unwrap();
    assert_eq!(typings.main(), Some("index.d.ts"));
    let options = render(&typings, Some("./dist/options.d.ts"), "left-pad 1.3.0").unwrap();
    assert!(options.contains("export interface Options"), "{options}");
    assert!(
        !options.contains("Other declaration files"),
        "one file asked for: {options}"
    );
    assert_eq!(render(&typings, Some("nope.d.ts"), "left-pad"), None);
}

#[test]
fn definitely_typed_names_scoped_packages_with_two_underscores() {
    assert_eq!(definitely_typed("node"), "@types/node");
    assert_eq!(definitely_typed("@babel/core"), "@types/babel__core");
}

#[test]
fn exports_follow_reexports_through_the_declaration_files() {
    let typings = Typings {
        declared: Some("./index.d.cts".into()),
        files: [
            (
                "index.d.cts",
                concat!(
                    "export * from \"./v4/external.cjs\";\n",
                    "export * as locales from \"./locales/index.cjs\";\n",
                ),
            ),
            (
                "v4/external.d.cts",
                concat!(
                    "/** Parses anything. */\n",
                    "export declare function parse(input: unknown): unknown;\n",
                    "export declare class ZodError extends Error {}\n",
                    "export interface ZodType { _type: unknown }\n",
                    "export type Infer<T> = T;\n",
                    "export declare const string: () => ZodType, number: () => ZodType;\n",
                    "declare function object(): ZodType;\n",
                    "export { object, object as obj };\n",
                    "export * from \"../shared\";\n",
                ),
            ),
            (
                "shared/index.d.ts",
                concat!(
                    "export declare enum Kind { A }\n",
                    "export * from \"../index.cjs\";\n",
                ),
            ),
        ]
        .into_iter()
        .map(|(path, text)| (path.to_owned(), text.to_owned()))
        .collect(),
    };

    let found: Vec<(String, String, Option<String>)> = exports(&typings, "zod")
        .into_iter()
        .map(|export| (export.path, export.kind, export.docs))
        .collect();
    let path = |p: &str, kind: &str| (format!("zod::{p}"), kind.to_owned(), None);
    assert_eq!(
        found,
        [
            (
                "zod::parse".to_owned(),
                "function".to_owned(),
                Some("Parses anything.".to_owned())
            ),
            path("ZodError", "class"),
            path("ZodType", "interface"),
            path("Infer", "type"),
            path("string", "variable"),
            path("number", "variable"),
            path("object", "function"),
            path("obj", "function"),
            path("Kind", "enum"),
            path("locales", "namespace"),
        ]
    );
}
