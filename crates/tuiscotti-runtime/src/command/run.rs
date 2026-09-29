use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use super::*;


/// How often the supervisor polls the child while waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(2);

impl Command {

    /// Export the spawn configuration as a [`std::process::Command`]
    /// (program, args, env, cwd, shell mapping). Stdin bytes, timeout,
    /// output limits, and the drain deadline are [`Command::run`] behavior
    /// and are not represented in the returned value.
    pub fn std_command(&self) -> std::process::Command {
        let mut cmd = match &self.program {
            Program::Direct(p) => std::process::Command::new(p),
            Program::CargoBin { name, resolved } => std::process::Command::new(
                resolved
                    .as_ref()
                    .map(|p| p.as_os_str())
                    .unwrap_or(name.as_os_str()),
            ),
        };
        if self.shell {
            // Rebuild as /bin/sh -c <script> sh <args...>: the program is the
            // script, builder args become positional parameters.
            let script = match &self.program {
                Program::Direct(p) => p.clone(),
                Program::CargoBin { name, resolved } => resolved
                    .as_ref()
                    .map(|p| p.as_os_str().to_os_string())
                    .unwrap_or_else(|_| name.clone()),
            };
            cmd = std::process::Command::new("/bin/sh");
            cmd.arg("-c").arg(script).arg("sh");
        }
        cmd.args(&self.args);
        if self.env_clear {
            cmd.env_clear();
        }
        for (k, v) in &self.env {
            match v {
                Some(v) => {
                    cmd.env(k, v);
                }
                None => {
                    cmd.env_remove(k);
                }
            }
        }
        if let Some(cwd) = &self.cwd {
            cmd.current_dir(cwd);
        }
        cmd
    }

    /// Spawn the child, collect output deadlock-safely, and return the result.
    ///
    /// stdout/stderr drain on dedicated threads while stdin is written, so a
    /// child filling both pipes (or a large stdin while pipes fill) cannot
    /// deadlock against buffer limits. Infallible: even spawn failure is
    /// data ([`Termination::SpawnError`] with [`ProcessOutput::error`]).
    pub fn run(&self) -> ProcessOutput {
        let start = Instant::now();
        let mut out = ProcessOutput {
            stdout: Vec::new(),
            stderr: Vec::new(),
            status: Termination::SpawnError,
            truncated: false,
            elapsed: Duration::ZERO,
            error: None,
        };
        if let Program::CargoBin {
            resolved: Err(e), ..
        } = &self.program
        {
            out.error = Some(e.clone());
            out.elapsed = start.elapsed();
            return out;
        }
        let mut cmd = self.std_command();
        cmd.stdin(if self.stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // `kill_on_drop` is intentionally NOT used: the supervisor below owns
        // the full lifecycle (timeout/limit kill, reap, bounded drain).
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                out.error = Some(SpawnError::spawn_failed(e.to_string()));
                out.elapsed = start.elapsed();
                return out;
            }
        };

        let limit_hit = Arc::new(AtomicBool::new(false));
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            out.error = Some(SpawnError::spawn_failed(
                "child stdio pipes unavailable after spawn".to_string(),
            ));
            out.elapsed = start.elapsed();
            return out;
        };
        let stdout_rx = spawn_drain(stdout, self.output_limit, Arc::clone(&limit_hit));
        let stderr_rx = spawn_drain(stderr, self.output_limit, Arc::clone(&limit_hit));
        if let Some(input) = self.stdin_bytes.clone() {
            let Some(mut stdin) = child.stdin.take() else {
                out.error = Some(SpawnError::spawn_failed(
                    "child stdin pipe unavailable after spawn".to_string(),
                ));
                out.elapsed = start.elapsed();
                return out;
            };
            std::thread::spawn(move || {
                // Broken pipe only means the child exited without reading
                // stdin; the child's termination status stays authoritative.
                let _ = stdin.write_all(&input);
            });
        }

        let deadline = self.timeout.map(|t| start + t);
        let status = loop {
            if limit_hit.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                out.truncated = true;
                break Termination::OutputLimit;
            }
            match child.try_wait() {
                Ok(Some(st)) => {
                    // A limit observed while the child exited concurrently
                    // still wins: bytes were dropped either way.
                    if limit_hit.load(Ordering::SeqCst) {
                        out.truncated = true;
                        break Termination::OutputLimit;
                    }
                    break classify(st);
                }
                Ok(None) => {}
                Err(e) => {
                    // try_wait failing after a live spawn should not happen;
                    // kill defensively and report what we know.
                    let _ = child.kill();
                    let _ = child.wait();
                    out.error = Some(SpawnError::wait_failed(e.to_string()));
                    break Termination::SpawnError;
                }
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                let _ = child.kill();
                let _ = child.wait();
                break Termination::Timeout;
            }
            std::thread::sleep(POLL_INTERVAL);
        };
        out.status = status;

        // Bounded late-output collection after the reap (R02): descendants
        // may still hold the pipes open, so each stream gets drain_deadline.
        let drain = self.drain_deadline;
        match stdout_rx.recv_timeout(drain) {
            Ok(bytes) => out.stdout = bytes,
            Err(_) => out.truncated = true,
        }
        match stderr_rx.recv_timeout(drain) {
            Ok(bytes) => out.stderr = bytes,
            Err(_) => out.truncated = true,
        }
        out.elapsed = start.elapsed();
        out
    }
}


#[cfg(unix)]
fn classify(status: ExitStatus) -> Termination {
    use std::os::unix::process::ExitStatusExt;
    if let Some(sig) = status.signal() {
        Termination::Signal(sig)
    } else {
        Termination::Exit(status.code().unwrap_or(-1))
    }
}


#[cfg(not(unix))]
fn classify(status: ExitStatus) -> Termination {
    // No signal reporting outside unix; do not invent one.
    Termination::Exit(status.code().unwrap_or(-1))
}


/// Drain one pipe on a thread; enforce the per-stream cap.
fn spawn_drain(
    mut pipe: impl Read + Send + 'static,
    limit: Option<usize>,
    limit_hit: Arc<AtomicBool>,
) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break, // EOF: all writers closed.
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(max) = limit {
                        if buf.len() > max {
                            buf.truncate(max);
                            limit_hit.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                }
                Err(_) => break, // Pipe error: return what we have.
            }
        }
        let _ = tx.send(buf);
    });
    rx
}
