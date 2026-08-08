//! --preview output mode: basic smoke test that a web server actually starts
//! and serves the rendered snapshot.

mod common;

use std::time::Duration;

use common::{Cmd, PLAIN, PREVIEW_NO_OPEN_ENV, extract_preview_url};

#[test]
fn preview_serves_snapshot() {
    // CONTRACT: --preview binds an OS-assigned port on 127.0.0.1, prints the
    // preview URL as a line on stdout, and does not open a browser when
    // TRMNL_CONSOLE_NO_OPEN=1 is set (see the main.rs "Output Modes" docs).
    let mut child = Cmd::new()
        .size(PLAIN.cols, PLAIN.rows)
        .args(["--preview"])
        .env(PREVIEW_NO_OPEN_ENV, "1")
        .stdin_bytes(PLAIN.ansi)
        .spawn();

    let line = child.expect_stdout_line(Duration::from_secs(10));
    let url = extract_preview_url(&line);

    let mut response = ureq::get(&url)
        .call()
        .unwrap_or_else(|e| panic!("GET {url} failed: {e}"));
    assert_eq!(response.status().as_u16(), 200);
    let content_type = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or("").to_string())
        .unwrap_or_default();
    assert!(
        content_type.contains("text/html"),
        "preview should serve HTML, got content-type {content_type:?}"
    );
    let body = response
        .body_mut()
        .read_to_string()
        .expect("reading preview body failed");
    // Loose: the page markup around the snapshot is unspecified; the terminal
    // content itself must be in there.
    assert!(
        body.contains("hi"),
        "preview page should contain the terminal content; body: {body:?}"
    );

    // The server runs until stopped; it must still be alive after serving.
    assert!(
        child.is_running(),
        "preview server exited prematurely; stderr may explain"
    );
    child.kill_and_finish();
}
