//! `trmnl-console-backend` integration tests: spawns the real binary and hits it over HTTP.
//!
//! The binary only exists when built with `--features backend` (`cargo test --features
//! backend`) - see `Cargo.toml`.

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Backend {
    child: Child,
    base_url: String,
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_backend(state_dir: &std::path::Path) -> Backend {
    let Ok(bin) = std::env::var("CARGO_BIN_EXE_trmnl-console-backend") else {
        panic!(
            "CARGO_BIN_EXE_trmnl-console-backend not set - build with `cargo test --features backend`"
        );
    };
    let port = free_port();
    let child = Command::new(bin)
        .env("BACKEND_BIND", format!("127.0.0.1:{port}"))
        .env("BACKEND_STATE_DIR", state_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn trmnl-console-backend");
    let base_url = format!("http://127.0.0.1:{port}");

    // Wait for it to actually be listening instead of a fixed sleep.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if ureq::get(format!("{base_url}/health")).call().is_ok() {
            break;
        }
        if Instant::now() > deadline {
            panic!("trmnl-console-backend did not start listening within 10s");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    Backend { child, base_url }
}

/// ureq errors (including non-2xx responses - `Error::StatusCode`) on anything but success,
/// so both match arms here are "the server actually answered", just via different paths.
macro_rules! status_and_body {
    ($result:expr) => {
        match $result {
            Ok(mut resp) => {
                let status = resp.status().as_u16();
                let body = resp.body_mut().read_to_string().unwrap_or_default();
                (status, body)
            }
            Err(ureq::Error::StatusCode(code)) => (code, String::new()),
            Err(err) => panic!("request failed: {err}"),
        }
    };
}

impl Backend {
    fn get(&self, path: &str) -> (u16, String) {
        status_and_body!(ureq::get(format!("{}{}", self.base_url, path)).call())
    }

    fn post(&self, path: &str, body: &str) -> (u16, String) {
        let req = ureq::post(format!("{}{}", self.base_url, path))
            .header("Content-Type", "application/json");
        status_and_body!(req.send(body))
    }
}

const AN_ID: &str = "a1b2c3d4-e5f6-4a5b-8c9d-0123456789ab";

#[test]
fn health_needs_no_id() {
    let dir = tempfile::tempdir().unwrap();
    let backend = start_backend(dir.path());
    let (status, body) = backend.get("/health");
    assert_eq!(status, 200);
    assert_eq!(body, "ok");
}

#[test]
fn poll_before_any_push_is_an_empty_object() {
    let dir = tempfile::tempdir().unwrap();
    let backend = start_backend(dir.path());
    let (status, body) = backend.get(&format!("/{AN_ID}"));
    assert_eq!(status, 200);
    assert_eq!(body, "{}");
}

#[test]
fn pushed_payload_is_served_unwrapped_on_the_same_url() {
    let dir = tempfile::tempdir().unwrap();
    let backend = start_backend(dir.path());

    let (status, _) = backend.post(
        &format!("/{AN_ID}"),
        r#"{"merge_variables":{"data":{"bar":{"left":"hi"},"variants":[{"id":"a","width":4,"scale":1,"content":"x"}]}}}"#,
    );
    assert_eq!(status, 200);

    let (status, body) = backend.get(&format!("/{AN_ID}"));
    assert_eq!(status, 200);
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        parsed,
        serde_json::json!({
            "data": {
                "bar": {"left": "hi"},
                "variants": [{"id": "a", "width": 4, "scale": 1, "content": "x"}]
            }
        })
    );
}

#[test]
fn different_ids_do_not_see_each_others_state() {
    let dir = tempfile::tempdir().unwrap();
    let backend = start_backend(dir.path());

    backend.post("/id-one", r#"{"merge_variables":{"data":{"bar":"one"}}}"#);
    backend.post("/id-two", r#"{"merge_variables":{"data":{"bar":"two"}}}"#);

    let (_, body_one) = backend.get("/id-one");
    let (_, body_two) = backend.get("/id-two");
    assert_eq!(body_one, r#"{"data":{"bar":"one"}}"#);
    assert_eq!(body_two, r#"{"data":{"bar":"two"}}"#);
}

#[test]
fn deep_merge_preserves_untouched_keys() {
    let dir = tempfile::tempdir().unwrap();
    let backend = start_backend(dir.path());

    backend.post(
        &format!("/{AN_ID}"),
        r#"{"merge_variables":{"data":{"bar":{"left":"hi","right":"there"}}}}"#,
    );
    backend.post(
        &format!("/{AN_ID}"),
        r#"{"merge_variables":{"data":{"bar":{"left":"updated"}}},"merge_strategy":"deep_merge"}"#,
    );

    let (_, body) = backend.get(&format!("/{AN_ID}"));
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        parsed,
        serde_json::json!({"data": {"bar": {"left": "updated", "right": "there"}}})
    );
}

#[test]
fn state_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();

    {
        let backend = start_backend(dir.path());
        backend.post(
            &format!("/{AN_ID}"),
            r#"{"merge_variables":{"data":{"bar":null}}}"#,
        );
        // give the write a moment to hit disk before we kill the process
        std::thread::sleep(Duration::from_millis(100));
    }

    let backend = start_backend(dir.path());
    let (status, body) = backend.get(&format!("/{AN_ID}"));
    assert_eq!(status, 200);
    assert_eq!(body, r#"{"data":{"bar":null}}"#);
}

#[test]
fn path_traversal_ids_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let backend = start_backend(dir.path());

    let (status, _) = backend.get("/../../etc/passwd");
    assert_ne!(status, 200);

    let (status, _) = backend.post("/../evil", r#"{"merge_variables":{}}"#);
    assert_ne!(status, 200);
}

#[test]
fn malformed_push_body_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let backend = start_backend(dir.path());
    let (status, _) = backend.post(&format!("/{AN_ID}"), r#"{"not_merge_variables":{}}"#);
    assert_eq!(status, 400);
}
