use super::*;
#[cfg(unix)]
use crate::proto::{
    SESSION_ENDPOINT_VERSION, SessionBackend, current_uid, now_unix, set_runtime_dir_override,
};
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
static CTR: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
fn scratch() -> std::path::PathBuf {
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("tuiscotti-admit-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("test dir");
    dir
}

#[cfg(unix)]
fn seed(dir: &Path, name: &str, pid: u32, owner: u32) {
    let ep = SessionEndpoint {
        version: SESSION_ENDPOINT_VERSION,
        name: name.to_string(),
        pid,
        argv: vec!["sleep".to_string()],
        backend: SessionBackend::Process,
        started_unix: now_unix(),
        owner,
        daemon_pid: None,
    };
    write_endpoint(dir, &ep).expect("seed endpoint");
}

#[cfg(unix)]
#[test]
fn piped_start_refuses_at_cap_and_cleans_up_below_it() {
    struct OverrideClear;
    impl Drop for OverrideClear {
        fn drop(&mut self) {
            set_runtime_dir_override(None);
        }
    }
    let dir = scratch();
    set_runtime_dir_override(Some(dir.clone()));
    let _clear = OverrideClear;
    let owner = current_uid().expect("uid");
    for i in 0..MAX_CONCURRENT_SESSIONS {
        seed(&dir, &format!("live-{i}"), std::process::id(), owner);
    }
    // Highest valid pid: dead on every platform (pid_max ≪ 2³¹−1).
    seed(&dir, "dead", 2_147_483_647, owner);
    std::fs::write(dir.join("foreign.txt"), b"x").expect("seed foreign");
    // Poison records: a pid-0 endpoint (invalid payload) and a directory
    // at an entry path. Both must occupy no slot and veto no start.
    std::fs::write(
        dir.join("poison.json"),
        format!(
            r#"{{"version":{SESSION_ENDPOINT_VERSION},"name":"poison","pid":0,"argv":["x"],"backend":"process","started_unix":{},"owner":{owner}}}"#,
            now_unix(),
        ),
    )
    .expect("seed poison");
    std::fs::create_dir(dir.join("dz.json")).expect("seed dir entry");
    // Full: typed rejection, nothing spawned or published.
    let before = std::fs::read_dir(&dir).expect("list dir").count();
    let argv = [OsString::from("/bin/sleep"), OsString::from("30")];
    let err = session_start_os("newbie", &argv, false).expect_err("full dir must refuse");
    assert_eq!(err.code, SESSION_LIMIT_CODE, "{err}");
    let after = std::fs::read_dir(&dir).expect("list dir").count();
    assert_eq!(after, before, "refused start spawns nothing");
    assert!(!dir.join("newbie.json").exists(), "nothing published");
    assert!(!dir.join("newbie.lock").exists(), "reservation released");
    // Dead records, foreign files, and poison entries occupy no slot;
    // the start below cleans up fully and preserves the poison records.
    std::fs::remove_file(dir.join("live-0.json")).expect("free a slot");
    let info = session_start_os("newbie", &argv, false).expect("slot reopens");
    assert!(pid_alive(info.pid), "started child must be alive");
    session_stop("newbie").expect("stop succeeds");
    assert!(!dir.join("newbie.json").exists(), "endpoint removed");
    assert!(!pid_alive(info.pid), "stray child survived");
    assert!(dir.join("poison.json").is_file(), "poison preserved");
    assert!(dir.join("dz.json").is_dir(), "dir entry preserved");
    if std::fs::remove_dir_all(&dir).is_err() {
        // Leftover scratch in the temp dir is harmless.
    }
}
