//! Contract tests for the shared bench driver CLI ([`parse_args`]/[`wants`]):
//! both harness binaries parse through here, so malformed flags and
//! missing `--out` must fail loudly instead of running a silent default.

use tuiscotti_bench::driver::{parse_args, wants};

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().copied().map(str::to_string).collect()
}

#[test]
fn parses_full_invocation() {
    let args = parse_args(
        &argv(&[
            "bench",
            "--scenario",
            "a,b",
            "--size",
            "all",
            "--samples",
            "7",
            "--workers",
            "2",
            "--out",
            "out.jsonl",
        ]),
        "help",
        100,
    )
    .expect("valid invocation must parse");
    assert_eq!(args.scenarios, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(args.sizes, vec![0, 1, 2]);
    assert_eq!(args.samples, 7);
    assert_eq!(args.workers, 2);
    assert_eq!(args.out.to_str(), Some("out.jsonl"));
    assert!(wants(&args, "a"));
    assert!(wants(&args, "b"));
    assert!(!wants(&args, "c"));
}

#[test]
fn defaults_select_all_scenarios_and_sizes() {
    let args = parse_args(&argv(&["bench", "--out", "o.jsonl"]), "help", 100)
        .expect("minimal invocation must parse");
    assert_eq!(args.scenarios, vec!["all".to_string()]);
    assert_eq!(args.sizes, vec![0, 1, 2]);
    assert_eq!(args.samples, 100);
    assert!(wants(&args, "anything"));
}

#[test]
fn rejects_malformed_invocations() {
    for words in [
        vec!["bench"],
        vec!["bench", "--out"],
        vec!["bench", "--bogus", "x", "--out", "o"],
        vec!["bench", "--samples", "nope", "--out", "o"],
        vec!["bench", "--size", "nope", "--out", "o"],
        vec!["bench", "--scenario", "a"],
    ] {
        let err = parse_args(&argv(&words), "help", 100)
            .expect_err("malformed invocation must be rejected");
        assert!(
            !err.to_string().is_empty(),
            "rejection must carry a message for {words:?}"
        );
    }
}
