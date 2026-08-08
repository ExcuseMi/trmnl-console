//! Input "stdin mode" (no COMMAND, stdin is not a TTY) with the default
//! HTML output mode: bytes from stdin drive the virtual terminal; the
//! snapshot is taken when stdin closes or --wait-time elapses.
//!
//! Expected HTML strings mirror the sbuffer.rs unit-test vectors, so these
//! pin the canonical encoder policy through the whole pipeline.

mod common;

use std::io::Write;
use std::time::{Duration, Instant};

use common::{Cmd, Fixture, assert_html, expected_stdout};

fn html_snapshot(f: &Fixture) {
    let out = Cmd::new().size(f.cols, f.rows).stdin_bytes(f.ansi).run();
    assert_eq!(
        out.code(),
        0,
        "stdout: {:?}, stderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(
        out.stderr.is_empty(),
        "no stderr expected on success: {}",
        out.stderr
    );
    // CONTRACT: HTML_TRAILING_NEWLINE (via assert_html).
    assert_html(&out, f);
}

#[test]
fn plain_text_html() {
    html_snapshot(&common::PLAIN);
}

#[test]
fn red_fg_html() {
    html_snapshot(&common::RED_FG);
}

#[test]
fn bg_bce_html() {
    html_snapshot(&common::BG_BCE);
}

#[test]
fn bold_toggle_html() {
    html_snapshot(&common::BOLD);
}

#[test]
fn two_rows_html() {
    html_snapshot(&common::TWO_ROWS);
}

#[test]
fn snapshot_on_stdin_close_without_wait_time() {
    let start = Instant::now();
    let out = Cmd::new().size(4, 2).stdin_bytes("hi").run();
    assert_eq!(out.code(), 0, "stderr: {}", out.stderr);
    // Generous bound — only proves the snapshot is triggered by stdin EOF
    // rather than some long default wait. Not a performance assertion.
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "snapshot after stdin close took {:?}",
        start.elapsed()
    );
}

#[test]
fn wait_time_cuts_off_slow_stdin() {
    let mut child = Cmd::new()
        .size(10, 1)
        .args(["--wait-time", "0.5"])
        .stdin_piped()
        .spawn();
    child.stdin().write_all(b"ab").unwrap();
    child.stdin().flush().unwrap();
    // stdin stays open: the snapshot must happen because --wait-time elapsed,
    // not because of EOF. The hard timeout backstops a hang.
    let out = child.wait_finish();
    assert_eq!(out.code(), 0, "stderr: {}", out.stderr);
    // "ab" padded to 10 columns.
    assert_eq!(out.stdout, expected_stdout("ab        "));
}
