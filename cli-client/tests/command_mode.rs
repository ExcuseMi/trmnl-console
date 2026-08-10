//! Input "command mode": a COMMAND runs inside the virtual terminal; the
//! snapshot is taken when it exits 0 or --wait-time elapses. Non-zero exits
//! propagate as trmnl-console's own exit code.

mod common;

use common::{Cmd, PLAIN, TWO_ROWS, assert_html};

#[test]
fn command_exit_zero_snapshots() {
    let out = Cmd::new()
        .size(4, 2)
        .args(["--", "sh", "-c", "printf hi"])
        .run();
    assert_eq!(out.code(), 0, "stderr: {}", out.stderr);
    assert_html(&out, &PLAIN);
}

#[test]
fn command_newlines_via_pty() {
    // CONTRACT: relies on the virtual PTY's ONLCR translating the command's
    // "\n" into "\r\n" (shadow-terminal default termios). If the
    // implementation configures raw termios, switch the printf input to an
    // explicit \r\n instead.
    let out = Cmd::new()
        .size(3, 2)
        .args(["--", "sh", "-c", "printf 'ab\\ncd'"])
        .run();
    assert_eq!(out.code(), 0, "stderr: {}", out.stderr);
    assert_html(&out, &TWO_ROWS);
}

#[test]
fn nonzero_exit_propagates() {
    let out = Cmd::new()
        .size(4, 2)
        .args(["--", "sh", "-c", "exit 7"])
        .run();
    assert_eq!(out.code(), 7, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.is_empty(),
        "an error message on stderr is expected"
    );
    assert!(
        out.stdout.is_empty(),
        "no snapshot on failed command; stdout: {:?}",
        out.stdout
    );
}

#[test]
fn wait_time_kills_and_snapshots() {
    // CONTRACT: WAIT_TIME_KILL_SNAPSHOTS — the kill at --wait-time expiry
    // must not be treated as the command "exiting with a non-zero exit code"
    // (a killed child never exits 0), otherwise this test contradicts
    // nonzero_exit_propagates. Flip the constant if the design changes.
    let out = Cmd::new()
        .size(4, 2)
        .args([
            "--wait-time",
            "0.5",
            "--",
            "sh",
            "-c",
            "printf hi; sleep 100",
        ])
        .run();
    if common::WAIT_TIME_KILL_SNAPSHOTS {
        assert_eq!(out.code(), 0, "stderr: {}", out.stderr);
        assert_html(&out, &PLAIN);
    } else {
        assert_ne!(out.code(), 0, "timeout-kill should be reported as an error");
    }
}

#[test]
fn stderr_is_forwarded() {
    // The command's stdout drives the terminal; its stderr is forwarded to
    // trmnl-console's own stderr.
    let out = Cmd::new()
        .size(6, 1)
        .args(["--", "sh", "-c", "printf err-marker >&2; printf ok"])
        .run();
    assert_eq!(out.code(), 0, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("err-marker"),
        "command stderr should be forwarded: {}",
        out.stderr
    );
    assert!(
        out.stdout.contains("ok"),
        "snapshot should contain the command's stdout: {:?}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("err-marker"),
        "command stderr must not leak into the terminal snapshot: {:?}",
        out.stdout
    );
}
