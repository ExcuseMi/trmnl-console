//! CLI-surface tests: clap-level behavior that already works against the
//! current stub. These validate the test harness itself and must stay green.

mod common;

use common::Cmd;

#[test]
fn missing_width_and_height_fail() {
    let out = Cmd::new().run();
    assert_eq!(out.code(), 2, "clap usage errors exit with 2");
    assert!(
        out.stderr.contains("--width"),
        "stderr should mention the missing --width: {}",
        out.stderr
    );
}

#[test]
fn missing_height_fails() {
    let out = Cmd::new().args(["-w", "10"]).run();
    assert_eq!(out.code(), 2);
    assert!(
        out.stderr.contains("--height"),
        "stderr should mention the missing --height: {}",
        out.stderr
    );
}

#[test]
fn scale_zero_rejected() {
    let out = Cmd::new().size(4, 2).args(["--scale", "0"]).run();
    assert_eq!(out.code(), 2);
    assert!(
        out.stderr.contains("1..=9"),
        "stderr should mention the valid range: {}",
        out.stderr
    );
}

#[test]
fn scale_ten_rejected() {
    let out = Cmd::new().size(4, 2).args(["--scale", "10"]).run();
    assert_eq!(out.code(), 2);
    assert!(out.stderr.contains("1..=9"), "stderr: {}", out.stderr);
}

#[test]
fn scale_nine_accepted() {
    // Only asserts clap accepts the value (exit != 2); whether the run
    // succeeds depends on the implemented pipeline, covered elsewhere.
    let out = Cmd::new()
        .size(4, 2)
        .args(["--scale", "9", "--json"])
        .stdin_bytes("")
        .run();
    assert_ne!(out.code(), 2, "scale 9 must parse; stderr: {}", out.stderr);
}

#[test]
fn url_json_conflict() {
    let out = Cmd::new()
        .size(4, 2)
        .args(["--url", "http://example.invalid/hook", "--json"])
        .run();
    assert_eq!(out.code(), 2);
    assert!(
        out.stderr.contains("cannot be used with"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn url_preview_conflict() {
    let out = Cmd::new()
        .size(4, 2)
        .args(["--url", "http://example.invalid/hook", "--preview"])
        .run();
    assert_eq!(out.code(), 2);
    assert!(
        out.stderr.contains("cannot be used with"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn json_preview_conflict() {
    let out = Cmd::new().size(4, 2).args(["--json", "--preview"]).run();
    assert_eq!(out.code(), 2);
    assert!(
        out.stderr.contains("cannot be used with"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn help_works() {
    let out = Cmd::new().args(["--help"]).run();
    assert_eq!(out.code(), 0, "stderr: {}", out.stderr);
    for needle in ["Input Modes", "Exit Codes", "tc--fg-X"] {
        assert!(
            out.stdout.contains(needle),
            "--help output should contain {needle:?}"
        );
    }
}

#[test]
fn version_works() {
    let out = Cmd::new().args(["--version"]).run();
    assert_eq!(out.code(), 0, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("trmnl-console"),
        "stdout: {}",
        out.stdout
    );
}

#[test]
fn command_after_double_dash_parses() {
    // Only asserts clap accepts a trailing command whose args contain flags;
    // command execution itself is covered in command_mode.rs.
    let out = Cmd::new()
        .size(4, 2)
        .args(["--json", "--", "sh", "-c", "true"])
        .run();
    assert_ne!(out.code(), 2, "stderr: {}", out.stderr);
}
