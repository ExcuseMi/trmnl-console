//! --url output mode against a mocked TRMNL API (httpmock).
//!
//! Wire format per https://docs.trmnl.com/go/private-plugins/webhooks:
//! POST, Content-Type: application/json, body {"merge_variables": {...}} with
//! an optional "merge_strategy". The exact split across the two requests
//! (metadata first, then content) is built by the CONTRACT body builders in
//! tests/common/mod.rs — tweak those, not the individual tests.

mod common;

use common::{
    Cmd, EXIT_WEBHOOK_FAILED, PLAIN, expected_content_body, expected_metadata_body,
};
use httpmock::prelude::*;

const HOOK_PATH: &str = "/api/custom_plugins/TEST-UUID";

fn run_against(url: String, extra_args: &[&str]) -> common::Out {
    Cmd::new()
        .size(PLAIN.cols, PLAIN.rows)
        .args(["--url", &url])
        .args(extra_args)
        .stdin_bytes(PLAIN.ansi)
        .run()
}

#[test]
fn webhook_happy_path() {
    // CONTRACT: body shapes come from the builders in common. If either
    // request's body deviates, it matches neither mock, httpmock answers 404,
    // and the exit-0 assertion below fails — inspect the mock server's
    // unmatched-request diagnostics in that case.
    let server = MockServer::start();
    let metadata = server.mock(|when, then| {
        when.method(POST)
            .path(HOOK_PATH)
            .header("content-type", "application/json")
            .json_body(expected_metadata_body(4, 1, None));
        then.status(200);
    });
    let content = server.mock(|when, then| {
        when.method(POST)
            .path(HOOK_PATH)
            .header("content-type", "application/json")
            .json_body(expected_content_body(PLAIN.sbuffer));
        then.status(200);
    });

    let out = run_against(server.url(HOOK_PATH), &[]);
    assert_eq!(
        out.code(),
        0,
        "stdout: {:?}, stderr: {}",
        out.stdout,
        out.stderr
    );
    metadata.assert();
    content.assert();
}

#[test]
fn webhook_metadata_sent_first() {
    // Order is proven without timestamps: the metadata request is rejected,
    // and the content mock must then have zero hits.
    // CONTRACT: assumes fail-fast — after a rejected request no further
    // request is sent.
    let server = MockServer::start();
    let metadata = server.mock(|when, then| {
        when.method(POST)
            .path(HOOK_PATH)
            .json_body(expected_metadata_body(4, 1, None));
        then.status(500);
    });
    let content = server.mock(|when, then| {
        when.method(POST)
            .path(HOOK_PATH)
            .json_body(expected_content_body(PLAIN.sbuffer));
        then.status(200);
    });

    let out = run_against(server.url(HOOK_PATH), &[]);
    assert_eq!(out.code(), EXIT_WEBHOOK_FAILED, "stderr: {}", out.stderr);
    metadata.assert();
    assert_eq!(
        content.hits(),
        0,
        "content must not be sent before (or after a failed) metadata request"
    );
}

#[test]
fn webhook_rejected_429() {
    // Rate limiting per TRMNL docs.
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST).path(HOOK_PATH);
        then.status(429);
    });

    let out = run_against(server.url(HOOK_PATH), &[]);
    assert_eq!(out.code(), EXIT_WEBHOOK_FAILED, "stderr: {}", out.stderr);
    assert!(!out.stderr.is_empty(), "an error message is expected");
    assert!(mock.hits() >= 1);
}

#[test]
fn webhook_rejected_4xx() {
    // CONTRACT: WEBHOOK_REJECT_STATUS — representative payload-rejection
    // status; any 4xx/5xx must yield exit 90.
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path(HOOK_PATH);
        then.status(common::WEBHOOK_REJECT_STATUS);
    });

    let out = run_against(server.url(HOOK_PATH), &[]);
    assert_eq!(out.code(), EXIT_WEBHOOK_FAILED, "stderr: {}", out.stderr);
    assert!(!out.stderr.is_empty(), "an error message is expected");
}

#[test]
fn webhook_connection_refused() {
    // Bind a port, note it, drop the listener: connecting will be refused.
    // (Tiny race: the OS could hand the freed port to someone else between
    // drop and connect — negligible in practice.)
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };

    let out = run_against(format!("http://127.0.0.1:{port}{HOOK_PATH}"), &[]);
    assert_eq!(out.code(), EXIT_WEBHOOK_FAILED, "stderr: {}", out.stderr);
    assert!(!out.stderr.is_empty(), "an error message is expected");
}

#[test]
fn webhook_scale_and_bar_in_metadata() {
    // Flag passthrough onto the wire: --scale and --bar-* land in the
    // metadata request.
    let server = MockServer::start();
    let metadata = server.mock(|when, then| {
        when.method(POST).path(HOOK_PATH).json_body(expected_metadata_body(
            4,
            2,
            Some(serde_json::json!({"left": "L", "right": null, "icon": null})),
        ));
        then.status(200);
    });
    let content = server.mock(|when, then| {
        when.method(POST)
            .path(HOOK_PATH)
            .json_body(expected_content_body(PLAIN.sbuffer));
        then.status(200);
    });

    let out = run_against(server.url(HOOK_PATH), &["--scale", "2", "--bar-left", "L"]);
    assert_eq!(out.code(), 0, "stderr: {}", out.stderr);
    metadata.assert();
    content.assert();
}
