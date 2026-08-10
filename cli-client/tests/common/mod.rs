//! Shared harness for the trmnl-console integration tests.
//!
//! These tests spawn the built binary (`CARGO_BIN_EXE_trmnl-console`) and pin the
//! CLI contract documented in `src/main.rs`. The capture/output logic is written
//! against this contract, so most test binaries are red until it is implemented;
//! `cli_surface` passes against the arg-parsing stub and validates this harness.
//!
//! Contract points that are not 100% settled live in the CONTRACT constants
//! below. Tests that depend on one carry a `// CONTRACT:` comment naming it.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

pub const BIN: &str = env!("CARGO_BIN_EXE_trmnl-console");

/// Hard ceiling for every wait in the suite, so a stuck (or unimplemented)
/// binary can never hang the test run. On expiry the child is killed and the
/// test panics with whatever output was captured.
pub const HARD_TIMEOUT: Duration = Duration::from_secs(30);

// =====================================================================
// CONTRACT CONSTANTS
//
// The parts of the CLI contract that are NOT fully settled yet. Tests read
// these; when the implementation nails down a behavior, tweak the constant
// here instead of hunting through the test files.
// =====================================================================

/// TWEAKABLE: Does the default HTML output mode append a trailing '\n' after
/// the `SBuffer::to_html` string (e.g. by using `println!`)?
pub const HTML_TRAILING_NEWLINE: bool = true;

/// TWEAKABLE: Value of the top-level "merge_strategy" field on webhook
/// request 1 (metadata). `None` = field omitted entirely; TRMNL's default
/// strategy is "replace", which wipes stale variables — plausibly what the
/// metadata request wants.
pub const WEBHOOK_REQ1_MERGE_STRATEGY: Option<&str> = Some("deep_merge");

/// TWEAKABLE: "merge_strategy" on webhook request 2 (content). "deep_merge"
/// so the content merges into the data object set by request 1.
pub const WEBHOOK_REQ2_MERGE_STRATEGY: Option<&str> = Some("deep_merge");

/// TWEAKABLE: A rejection status TRMNL plausibly sends for oversized payloads.
/// Any 4xx/5xx must yield exit 90; this is just the representative non-429
/// value used by tests.
pub const WEBHOOK_REJECT_STATUS: u16 = 413;

/// TWEAKABLE: When --wait-time expires and the command is killed, the main.rs
/// doc says a snapshot IS taken. The kill must therefore not be treated as
/// the command "exiting with a non-zero exit code" (a killed child never
/// exits 0). `true` = snapshot on stdout + exit 0.
pub const WAIT_TIME_KILL_SNAPSHOTS: bool = true;

/// Env var that suppresses opening a browser tab in --preview mode
/// (documented in the main.rs "Output Modes" section).
pub const PREVIEW_NO_OPEN_ENV: &str = "TRMNL_CONSOLE_NO_OPEN";

/// Exit code for "TRMNL did not accept the webhook payload / network error".
pub const EXIT_WEBHOOK_FAILED: i32 = 90;

/// Exit code for "trmnl-console encountered an error" (e.g. command not found).
pub const EXIT_ERROR: i32 = 91;

// =====================================================================
// Webhook body builders — the expected wire shape, defined exactly once.
// See https://docs.trmnl.com/go/private-plugins/webhooks for the format.
// =====================================================================

/// Expected body of webhook request 1 (metadata: everything except content).
pub fn expected_metadata_body(
    width: u16,
    scale: u8,
    bar: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "merge_variables": {
            "data": {
                "width": width,
                "scale": scale,
                "bar": bar.unwrap_or(serde_json::Value::Null),
            }
        }
    });
    if let Some(strategy) = WEBHOOK_REQ1_MERGE_STRATEGY {
        body["merge_strategy"] = strategy.into();
    }
    body
}

/// Expected body of webhook request 2 (the SBuffer content).
pub fn expected_content_body(sbuffer: &str) -> serde_json::Value {
    let mut body = serde_json::json!({
        "merge_variables": {
            "data": {
                "content": sbuffer,
            }
        }
    });
    if let Some(strategy) = WEBHOOK_REQ2_MERGE_STRATEGY {
        body["merge_strategy"] = strategy.into();
    }
    body
}

// =====================================================================
// ANSI fixtures — mirrored from the sbuffer.rs unit-test vectors so the
// integration suite pins the same canonical encoder policy end-to-end.
// =====================================================================

pub struct Fixture {
    /// Bytes fed to the virtual terminal (via stdin, or printf'd in command mode).
    pub ansi: &'static str,
    pub cols: u16,
    pub rows: u16,
    /// Expected `SBuffer::to_html` output (default output mode, --preview page).
    pub html: &'static str,
    /// Expected SBuffer string (data.content in --json / webhook payloads).
    pub sbuffer: &'static str,
}

/// Plain text — mirrors `plain_text` / `plain_text_is_padded_to_width`.
pub const PLAIN: Fixture = Fixture {
    ansi: "hi",
    cols: 4,
    rows: 2,
    html: "hi  \n    ",
    sbuffer: "\u{E000}hi\n\n",
};

/// Red fg dangling over padding — mirrors `fg_span_open_at_eof_contains_padding`.
pub const RED_FG: Fixture = Fixture {
    ansi: "\x1b[31mab",
    cols: 4,
    rows: 1,
    html: "<span class=\"tc--fg-1\">ab  </span>",
    sbuffer: "\u{E000}\u{E101}ab\n",
};

/// BCE background padding — mirrors `bg_span_contains_row_padding`.
/// The `sbuffer` vector is derived from the spec, not copied from a unit test
/// (those use other sizes) — pin it against the real encoder once green.
pub const BG_BCE: Fixture = Fixture {
    ansi: "\x1b[44m\x1b[K",
    cols: 4,
    rows: 1,
    html: "<span class=\"tc--bg-4\">    </span>",
    sbuffer: "\u{E000}\u{E204}\n",
};

/// Bold toggle — mirrors `bold_toggles_on_and_off` / `bold_span`.
pub const BOLD: Fixture = Fixture {
    ansi: "\x1b[1mB\x1b[0mn",
    cols: 6,
    rows: 1,
    html: "<span class=\"tc--bold\">B</span>n    ",
    sbuffer: "\u{E000}\u{E402}B\u{E402}n\n",
};

/// Two rows — the explicit `\r\n` matters: raw bytes fed in stdin mode get no
/// ONLCR translation, so a bare `\n` would only move down, not to column 0.
pub const TWO_ROWS: Fixture = Fixture {
    ansi: "ab\r\ncd",
    cols: 3,
    rows: 2,
    html: "ab \ncd ",
    sbuffer: "\u{E000}ab\ncd\n",
};

/// The stdout expected for a given to_html string, honoring the
/// HTML_TRAILING_NEWLINE contract constant.
pub fn expected_stdout(html: &str) -> String {
    if HTML_TRAILING_NEWLINE {
        format!("<pre>{html}</pre>\n")
    } else {
        format!("<pre>{html}</pre>")
    }
}

/// Assert that stdout is exactly the fixture's HTML representation.
pub fn assert_html(out: &Out, f: &Fixture) {
    assert_eq!(
        out.stdout,
        expected_stdout(f.html),
        "HTML output mismatch ({}x{} terminal, input {:?}); stderr: {}",
        f.cols,
        f.rows,
        f.ansi,
        out.stderr,
    );
}

/// Extract the preview URL from the line the binary prints on stdout.
pub fn extract_preview_url(line: &str) -> String {
    let start = line
        .find("http://127.0.0.1:")
        .unwrap_or_else(|| panic!("no preview URL found in stdout line: {line:?}"));
    line[start..]
        .split_whitespace()
        .next()
        .unwrap()
        .trim_end_matches('.')
        .to_string()
}

// =====================================================================
// Spawn helpers
// =====================================================================

enum StdinMode {
    /// /dev/null — not a TTY, so demo mode cannot trigger accidentally.
    Null,
    /// Write these bytes, then close the pipe (EOF).
    Bytes(Vec<u8>),
    /// Keep the pipe open; the test drives it via `RunningChild::stdin()`.
    Piped,
}

/// Builder around `std::process::Command` for the trmnl-console binary.
pub struct Cmd {
    cmd: Command,
    stdin: StdinMode,
}

impl Cmd {
    pub fn new() -> Self {
        Self {
            cmd: Command::new(BIN),
            stdin: StdinMode::Null,
        }
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.cmd.args(args);
        self
    }

    /// Convenience for the required `-w`/`-h` args.
    pub fn size(mut self, width: u16, height: u16) -> Self {
        self.cmd
            .args(["-w", &width.to_string(), "-h", &height.to_string()]);
        self
    }

    /// Per-Command env only — tests must never mutate the test process env
    /// (test binaries run their tests in parallel threads).
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.cmd.env(key, value);
        self
    }

    pub fn stdin_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = StdinMode::Bytes(bytes.into());
        self
    }

    pub fn stdin_piped(mut self) -> Self {
        self.stdin = StdinMode::Piped;
        self
    }

    /// Spawn for long-running children (preview server, slow stdin writers).
    /// Reader threads are started immediately so the child can never fill the
    /// pipe buffer and deadlock against a later wait.
    pub fn spawn(mut self) -> RunningChild {
        self.cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        self.cmd.stdin(match self.stdin {
            StdinMode::Null => Stdio::null(),
            _ => Stdio::piped(),
        });
        let mut child = self.cmd.spawn().expect("failed to spawn trmnl-console");

        let (stdout_lines, stdout_all) = spawn_stdout_reader(child.stdout.take().unwrap());
        let stderr_all = spawn_string_reader(child.stderr.take().unwrap());

        let mut stdin = child.stdin.take();
        if let StdinMode::Bytes(bytes) = self.stdin {
            let mut handle = stdin.take().unwrap();
            // Writer thread: write everything, then drop the handle => EOF.
            std::thread::spawn(move || {
                let _ = handle.write_all(&bytes);
            });
        }

        RunningChild {
            child: Some(child),
            stdout_lines,
            stdout_all: Some(stdout_all),
            stderr_all: Some(stderr_all),
            stdin,
            started: Instant::now(),
        }
    }

    /// Spawn, close stdin after any configured bytes, wait for exit (with the
    /// hard timeout) and collect all output.
    pub fn run(self) -> Out {
        let mut child = self.spawn();
        child.close_stdin();
        child.wait_finish()
    }
}

pub struct Out {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    pub fn code(&self) -> i32 {
        self.status
            .code()
            .unwrap_or_else(|| panic!("child was terminated by a signal ({:?})", self.status))
    }

    pub fn assert_ok(self) -> Self {
        assert_eq!(
            self.code(),
            0,
            "expected exit 0;\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.stdout,
            self.stderr
        );
        self
    }
}

/// A spawned trmnl-console with live output readers and a kill-on-drop guard.
pub struct RunningChild {
    child: Option<Child>,
    stdout_lines: mpsc::Receiver<String>,
    stdout_all: Option<JoinHandle<String>>,
    stderr_all: Option<JoinHandle<String>>,
    stdin: Option<ChildStdin>,
    started: Instant,
}

impl RunningChild {
    fn remaining(&self) -> Duration {
        HARD_TIMEOUT.saturating_sub(self.started.elapsed())
    }

    pub fn stdin(&mut self) -> &mut ChildStdin {
        self.stdin
            .as_mut()
            .expect("stdin is not piped (or was already closed)")
    }

    /// Drop the stdin handle so the child sees EOF. No-op for Null/Bytes.
    pub fn close_stdin(&mut self) {
        self.stdin.take();
    }

    pub fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .unwrap()
            .try_wait()
            .expect("try_wait failed")
            .is_none()
    }

    /// Wait for the next full line on stdout, bounded by `timeout` and the
    /// remaining hard-timeout budget.
    pub fn expect_stdout_line(&mut self, timeout: Duration) -> String {
        let timeout = timeout.min(self.remaining());
        match self.stdout_lines.recv_timeout(timeout) {
            Ok(line) => line,
            Err(_) => {
                let out = self.kill_and_collect();
                panic!(
                    "no stdout line within {timeout:?};\n--- stdout so far ---\n{}\n--- stderr ---\n{}",
                    out.0, out.1
                );
            }
        }
    }

    /// Wait for natural exit within the remaining hard-timeout budget.
    pub fn wait_finish(mut self) -> Out {
        let mut child = self.child.take().unwrap();
        match child
            .wait_timeout(self.remaining())
            .expect("waiting for child failed")
        {
            Some(status) => Out {
                status,
                stdout: self.stdout_all.take().unwrap().join().unwrap(),
                stderr: self.stderr_all.take().unwrap().join().unwrap(),
            },
            None => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = self.stdout_all.take().unwrap().join().unwrap();
                let stderr = self.stderr_all.take().unwrap().join().unwrap();
                panic!(
                    "child did not exit within {HARD_TIMEOUT:?};\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
                );
            }
        }
    }

    /// Kill the child (e.g. a preview server that runs until stopped) and
    /// collect its output.
    pub fn kill_and_finish(mut self) -> Out {
        let mut child = self.child.take().unwrap();
        let _ = child.kill();
        let status = child.wait().expect("waiting for killed child failed");
        Out {
            status,
            stdout: self.stdout_all.take().unwrap().join().unwrap(),
            stderr: self.stderr_all.take().unwrap().join().unwrap(),
        }
    }

    fn kill_and_collect(&mut self) -> (String, String) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let stdout = self
            .stdout_all
            .take()
            .map(|h| h.join().unwrap())
            .unwrap_or_default();
        let stderr = self
            .stderr_all
            .take()
            .map(|h| h.join().unwrap())
            .unwrap_or_default();
        (stdout, stderr)
    }
}

impl Drop for RunningChild {
    /// A panicking test must not leak a running preview server / sleep child.
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Reader thread that pushes complete lines into a channel as they arrive and
/// returns the full raw stdout (byte-exact, trailing newlines preserved) on join.
fn spawn_stdout_reader(
    mut reader: impl Read + Send + 'static,
) -> (mpsc::Receiver<String>, JoinHandle<String>) {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut all: Vec<u8> = Vec::new();
        let mut line_start = 0usize;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    all.extend_from_slice(&buf[..n]);
                    while let Some(pos) = all[line_start..].iter().position(|&b| b == b'\n') {
                        let line = String::from_utf8_lossy(&all[line_start..line_start + pos])
                            .into_owned();
                        let _ = tx.send(line);
                        line_start += pos + 1;
                    }
                }
            }
        }
        String::from_utf8_lossy(&all).into_owned()
    });
    (rx, handle)
}

fn spawn_string_reader(mut reader: impl Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => all.extend_from_slice(&buf[..n]),
            }
        }
        String::from_utf8_lossy(&all).into_owned()
    })
}
