//! `trmnl-console-pusher --once` against a mocked TRMNL API (httpmock).
//!
//! Pins the multi-variant payload shape documented in `src/payload.rs`
//! (`WebhookPayloadDataMulti::into_webhook`): a single POST per job containing
//! `merge_variables.data.{bar,variants}`, where each variant carries its own
//! id/width/scale/content.

mod common;

use common::PLAIN;
use httpmock::prelude::*;
use std::io::Write;
use std::process::Command;
use std::time::Duration;
use wait_timeout::ChildExt;

const PUSHER_BIN: &str = env!("CARGO_BIN_EXE_trmnl-console-pusher");
const HOOK_PATH: &str = "/api/custom_plugins/TEST-UUID";

fn write_config(contents: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("failed to create temp config file");
    file.write_all(contents.as_bytes())
        .expect("failed to write temp config file");
    file
}

fn run_once(config_path: &std::path::Path) -> common::Out {
    let mut child = Command::new(PUSHER_BIN)
        .args(["--config", config_path.to_str().unwrap(), "--once"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn trmnl-console-pusher");

    let status = child
        .wait_timeout(Duration::from_secs(30))
        .expect("waiting for child failed")
        .unwrap_or_else(|| {
            let _ = child.kill();
            child.wait().expect("waiting for killed child failed");
            panic!("trmnl-console-pusher did not exit within 30s");
        });

    use std::io::Read;
    let mut stdout = String::new();
    let mut stderr = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();

    common::Out {
        status,
        stdout,
        stderr,
    }
}

#[test]
fn once_sends_single_variant_payload() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path(HOOK_PATH)
            .json_body(serde_json::json!({
                "merge_variables": {
                    "data": {
                        "bar": serde_json::Value::Null,
                        "variants": [
                            {
                                "id": "test-size",
                                "width": PLAIN.cols,
                                "scale": 1,
                                "content": PLAIN.sbuffer,
                            }
                        ]
                    }
                }
            }));
        then.status(200);
    });

    let config = write_config(&format!(
        r#"
webhook_url: "{url}"
jobs:
  - name: test-job
    command: "printf hi"
    interval_seconds: 60
    sizes:
      - {{ id: test-size, width: {width}, height: {height}, scale: 1 }}
"#,
        url = server.url(HOOK_PATH),
        width = PLAIN.cols,
        height = PLAIN.rows,
    ));

    let out = run_once(config.path());
    assert_eq!(
        out.code(),
        0,
        "stdout: {:?}, stderr: {}",
        out.stdout,
        out.stderr
    );
    mock.assert();
}

#[test]
fn once_reports_nonzero_when_no_runnable_jobs() {
    let config = write_config(
        r#"
jobs:
  - name: no-webhook
    command: "printf hi"
    interval_seconds: 60
    sizes:
      - { id: x, width: 4, height: 2 }
"#,
    );

    let out = run_once(config.path());
    assert_ne!(out.code(), 0, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("no webhook_url"),
        "expected a webhook_url complaint, got stderr: {}",
        out.stderr
    );
}
