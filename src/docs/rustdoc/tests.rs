use super::super::test_support::state_machines;
use super::*;

#[test]
fn the_outline_lists_modules_items_and_reexports() {
    let outline = render(&state_machines(), None).unwrap();
    for expected in [
        "# state_machines 0.32.1",
        "Type-safe state machines for Rust.",
        "## Modules\n\n- `runtime`: Optional, executor-independent event processing",
        "## Macros\n\n- `abort_with`: Abort an around callback",
        "## Structs\n\n- `Runner`: Explicit FIFO deferral rules",
        "## Re-exports\n\n- `pub use state_machines_core::GuardError`",
    ] {
        assert!(
            outline.contains(expected),
            "missing {expected:?} in:\n{outline}"
        );
    }
}

#[test]
fn items_are_found_with_or_without_the_crate_name() {
    let doc = state_machines();
    let runner = render(&doc, Some("runtime::Runner")).unwrap();
    assert_eq!(
        render(&doc, Some("state_machines::runtime::Runner")).as_ref(),
        Some(&runner)
    );
    for expected in [
        "# Struct `state_machines::runtime::Runner`",
        "```rust\nstruct Runner<M>\n```",
        "## Fields\n\n- `machine: M`: The machine it drives.",
        "- `fn dispatch<E>(&mut self, event: E) -> Result<(), RunError>`: Queue `event`",
        "## Implements\n\n`Debug`",
    ] {
        assert!(
            runner.contains(expected),
            "missing {expected:?} in:\n{runner}"
        );
    }

    let clock = render(&doc, Some("runtime::Clock")).unwrap();
    assert!(
        clock.contains("- `fn now(&self) -> u64`: The current tick."),
        "{clock}"
    );
    let function = render(&doc, Some("runtime::run_child")).unwrap();
    assert!(
        function.contains("fn run_child<M>(child: M) -> Result<M, RunError>"),
        "{function}"
    );
    assert_eq!(render(&doc, Some("runtime::Nope")), None);
}

#[test]
fn docs_rs_json_comes_zstd_compressed() {
    let json = state_machines().to_string();
    let zst = ruzstd::encoding::compress_to_vec(
        json.as_bytes(),
        ruzstd::encoding::CompressionLevel::Fastest,
    );
    assert_eq!(decode(&zst).unwrap(), state_machines());
    assert!(decode(b"not zstd").is_err());
}
