//! Contract tests for [`tuiscotti_core::names::validate_name`]: the shared
//! scenario/store name validator every store root depends on for
//! containment. Anything accepted here becomes path segments under a
//! store root, so the reject table is a traversal guard, not style.

use tuiscotti_core::names::validate_name;

#[test]
fn accepts_relative_nested_names() {
    for name in [
        "snap",
        "suite/snap",
        "a/b/c",
        "shot-01.dark",
        "v2/final snap",
    ] {
        assert!(validate_name(name).is_ok(), "must accept {name:?}");
    }
}

#[test]
fn rejects_escape_and_malformed_names() {
    for name in [
        "",
        "/abs",
        "a/../b",
        "..",
        ".",
        "a/./b",
        "a//b",
        "/",
        "a\\b",
        "\\server\\share",
    ] {
        assert!(validate_name(name).is_err(), "must reject {name:?}");
    }
}

#[test]
fn rejection_names_the_offending_input() {
    let err = validate_name("a/../b").expect_err("must reject parent segments");
    assert!(
        err.to_string().contains("a/../b"),
        "error must identify the input, got: {err}"
    );
}
