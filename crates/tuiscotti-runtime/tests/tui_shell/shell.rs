//! R12 shell sessions + command markers (split from `tui_shell.rs`; shared helpers live in the root).

use super::{cancel, contains, deadline};
use tuiscotti_runtime::tui::Tui;
use tuiscotti_runtime::tui_shell::{Markers, Shell, ShellError};

// ---------------------------------------------------------------------------
// R12: shell sessions + command markers
// ---------------------------------------------------------------------------

#[test]
fn shell_run_echo() {
    let shell = Shell::sh().expect("sh succeeds");
    assert_eq!(shell.markers(), Markers::Available);
    let r = shell
        .run("echo hello-42", deadline(10))
        .expect("run succeeds");
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.markers, Markers::Available);
    assert!(!r.truncated);
    assert!(r.output_span.iter().any(|l| l.contains("hello-42")));
    shell.into_session().close().expect("close succeeds");
}

#[test]
fn shell_cmd_exit_is_not_child_exit() {
    let shell = Shell::sh().expect("sh succeeds");
    let r = shell.run("(exit 3)", deadline(10)).expect("run succeeds");
    assert_eq!(r.exit_code, 3);
    // The shell (direct child) is still alive: exit 3 was the command's.
    assert!(shell.session().poll_exit().is_none());
    let r = shell.run("false", deadline(10)).expect("run succeeds");
    assert_eq!(r.exit_code, 1);
    let r = shell
        .run("printf 'a\\nb\\nc\\n'", deadline(10))
        .expect("run succeeds");
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.output_span, vec!["a", "b", "c"]);
    assert!(!r.truncated);
    shell.into_session().close().expect("close succeeds");
}

#[test]
fn shell_rejects_bad_commands() {
    let shell = Shell::sh().expect("sh succeeds");
    assert!(matches!(
        shell.run("", deadline(5)),
        Err(ShellError::BadCommand(_))
    ));
    assert!(matches!(
        shell.run("echo a\necho b", deadline(5)),
        Err(ShellError::BadCommand(_))
    ));
    shell.into_session().close().expect("close succeeds");
}

#[test]
fn shell_unavailable_refuses_to_guess() {
    let session = Tui::new(["/bin/cat"])
        .size(40, 10)
        .spawn()
        .expect("spawn succeeds");
    let mut shell = Shell::wrap(session);
    assert_eq!(shell.markers(), Markers::Unavailable);
    // No integration: run refuses instead of inferring spans from echo text.
    assert!(matches!(
        shell.run("echo hi", deadline(5)),
        Err(ShellError::NoIntegration(_))
    ));
    // Handshake against cat echoes but never confirms: bounded failure.
    assert!(shell.setup(deadline(2)).is_err());
    assert_eq!(shell.markers(), Markers::Unavailable);
    shell.into_session().close().expect("close succeeds");
}

#[test]
fn shell_truncation_flag() {
    let shell = Shell::sh_sized(80, 6).expect("sh_sized succeeds");
    let r = shell
        .run(
            "awk 'BEGIN{for(i=1;i<=30;i++)print \"line\"i}'",
            deadline(10),
        )
        .expect("run succeeds");
    assert_eq!(r.exit_code, 0);
    assert!(r.truncated, "start marker scrolled off: {r:?}");
    assert!(r.output_span.len() <= 6, "span: {:?}", r.output_span);
    assert_eq!(r.output_span.last().map(String::as_str), Some("line30"));
    shell.into_session().close().expect("close succeeds");
}

#[test]
fn shell_final_state_preserved_after_exit() {
    let shell = Shell::sh().expect("sh succeeds");
    // `exit` kills the shell before the end attestation: the run times out.
    assert!(shell.run("exit 0", deadline(3)).is_err());
    let waited = shell
        .wait_shell_exit(deadline(10), &cancel())
        .expect("wait_shell_exit succeeds");
    assert!(waited.status.success());
    // Final grid + state survive the child.
    assert!(contains(&waited.observation.screen, "__TUISCOTTI_C__").expect("screen rows readable"));
    shell.into_session().close().expect("close succeeds");
}
