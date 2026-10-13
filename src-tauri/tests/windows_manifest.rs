// This integration test exists so the package declares a test target: cargo
// rejects the `cargo:rustc-link-arg-tests` instruction emitted by `build.rs`
// (via embed-resource's `compile_for_tests`) when a package has no `tests/`
// targets. The manifest embedded through that instruction lets Windows test
// harnesses bind the Common-Controls v6 side-by-side assembly instead of the
// v5 copy in System32 that lacks `TaskDialogIndirect` (see build.rs).
#[test]
fn smoke() {
    assert!(true);
}