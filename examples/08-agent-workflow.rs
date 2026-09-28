//! 08: agent workflow — typed ops + machine JSON, no PTY needed.
//!
//! Run: `cargo run --example 08-agent-workflow`
//!
//! Agents drive the same `Op` vocabulary two ways: linked Rust (`execute`)
//! and newline-delimited JSON (`run_machine_line`, the `--machine` protocol).
//! Feature-independent ops (version/capabilities/assert/render/diff) work in
//! every build; PTY ops need the `pty` feature.

use tuisnap::proto::{capabilities, execute, run_machine_line, Op, OpResult, PROTOCOL_VERSION};

fn main() {
    // Linked Rust: version + a passing shared-engine check.
    match execute(&Op::Version).unwrap() {
        OpResult::Version { protocol, .. } => assert_eq!(protocol, PROTOCOL_VERSION),
        other => panic!("expected Version, got {other:?}"),
    }
    match execute(&Op::Assert {
        check: "text-contains".to_string(),
        text: Some("hello world".to_string()),
        needle: Some("world".to_string()),
        actual: None,
        expected: None,
    })
    .unwrap()
    {
        OpResult::Asserted { passed, .. } => assert!(passed),
        other => panic!("expected Asserted, got {other:?}"),
    }
    assert_eq!(capabilities().protocol, PROTOCOL_VERSION);

    // Machine JSON: one op line in, one envelope line out.
    let (line, ok) = run_machine_line(r#"{"type":"capabilities"}"#);
    assert!(ok);
    assert!(line.contains(r#""ok":true"#));
    assert!(line.contains(PROTOCOL_VERSION));
    // Adversarial input never panics: a typed error envelope instead.
    let (bad_line, bad_ok) = run_machine_line("not json");
    assert!(!bad_ok);
    assert!(bad_line.contains("invalid-input"));

    println!("EXAMPLE-08-OK protocol={PROTOCOL_VERSION} machine_ok={ok}");
}
