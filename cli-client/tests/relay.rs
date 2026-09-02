//! `trmnl-console-relay` integration tests: spawns the real binary and hits it over HTTP.
//!
//! Requires the `relay` feature (this test file is only compiled when it's enabled - see
//! `[[test]]` below is unnecessary since Cargo runs all `tests/*.rs` regardless of feature,
//! but the binary itself only exists when built with `--features relay`, so these tests are
//! skipped gracefully if the binary wasn't built that way).

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Relay {
    child: Child,
    base_url: String,
}

impl Drop for Relay {
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

fn start_relay(token: Option<&str>, ip_allowlist: bool, state_path: &std::path::Path) -> Relay {
    let Ok(bin) = std::env::var("CARGO_BIN_EXE_trmnl-console-relay") else {
        panic!(
            "CARGO_BIN_EXE_trmnl-console-relay not set - build with `cargo test --features relay`"
        );
    };
    let port = free_port();
    let mut cmd = Command::new(bin);
    cmd.env("RELAY_BIND", format!("127.0.0.1:{port}"))
        .env("RELAY_STATE_PATH", state_path)
        .env("RELAY_IP_ALLOWLIST", ip_allowlist.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(token) = token {
        cmd.env("RELAY_TOKEN", token);
    } else {
        cmd.env_remove("RELAY_TOKEN");
    }
    let child = cmd.spawn().expect("failed to spawn trmnl-console-relay");
    let base_url = format!("http://127.0.0.1:{port}");

    // Wait for it to actually be listening instead of a fixed sleep.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if ureq::get(&format!("{base_url}/health")).call().is_ok() {
            break;
        }
        if Instant::now() > deadline {
            panic!("trmnl-console-relay did not start listening within 10s");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    Relay { child, base_url }
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

impl Relay {
    fn get(&self, path: &str) -> (u16, String) {
        status_and_body!(ureq::get(format!("{}{}", self.base_url, path)).call())
    }

    fn post(&self, path: &str, bearer: Option<&str>, body: &str) -> (u16, String) {
        let mut req = ureq::post(format!("{}{}", self.base_url, path))
            .header("Content-Type", "application/json");
        if let Some(bearer) = bearer {
            req = req.header("Authorization", format!("Bearer {bearer}"));
        }
        status_and_body!(req.send(body))
    }
}

#[test]
fn health_is_always_open() {
    let dir = tempfile::tempdir().unwrap();
    let relay = start_relay(Some("secret"), false, &dir.path().join("state.json"));
    let (status, body) = relay.get("/health");
    assert_eq!(status, 200);
    assert_eq!(body, "ok");
}

#[test]
fn poll_before_any_push_is_an_empty_object() {
    let dir = tempfile::tempdir().unwrap();
    let relay = start_relay(Some("secret"), false, &dir.path().join("state.json"));
    let (status, body) = relay.get("/");
    assert_eq!(status, 200);
    assert_eq!(body, "{}");
}

#[test]
fn push_requires_the_correct_bearer_token() {
    let dir = tempfile::tempdir().unwrap();
    let relay = start_relay(Some("secret"), false, &dir.path().join("state.json"));
    let body = r#"{"merge_variables":{"data":{"bar":null}}}"#;

    let (status, _) = relay.post("/", None, body);
    assert_eq!(status, 401, "no token should be rejected");

    let (status, _) = relay.post("/", Some("wrong"), body);
    assert_eq!(status, 401, "wrong token should be rejected");

    let (status, _) = relay.post("/", Some("secret"), body);
    assert_eq!(status, 200, "correct token should be accepted");
}

#[test]
fn push_without_configured_token_is_always_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let relay = start_relay(None, false, &dir.path().join("state.json"));
    let (status, _) = relay.post("/", Some("anything"), r#"{"merge_variables":{"data":{}}}"#);
    assert_eq!(status, 503);
}

#[test]
fn pushed_payload_is_served_unwrapped_on_poll() {
    let dir = tempfile::tempdir().unwrap();
    let relay = start_relay(Some("secret"), false, &dir.path().join("state.json"));

    let (status, _) = relay.post(
        "/",
        Some("secret"),
        r#"{"merge_variables":{"data":{"bar":{"left":"hi"},"variants":[{"id":"a","width":4,"scale":1,"content":"x"}]}}}"#,
    );
    assert_eq!(status, 200);

    let (status, body) = relay.get("/");
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
fn deep_merge_preserves_untouched_keys() {
    let dir = tempfile::tempdir().unwrap();
    let relay = start_relay(Some("secret"), false, &dir.path().join("state.json"));

    relay.post(
        "/",
        Some("secret"),
        r#"{"merge_variables":{"data":{"bar":{"left":"hi","right":"there"}}}}"#,
    );
    relay.post(
        "/",
        Some("secret"),
        r#"{"merge_variables":{"data":{"bar":{"left":"updated"}}},"merge_strategy":"deep_merge"}"#,
    );

    let (_, body) = relay.get("/");
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        parsed,
        serde_json::json!({"data": {"bar": {"left": "updated", "right": "there"}}})
    );
}

#[test]
fn state_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");

    {
        let relay = start_relay(Some("secret"), false, &state_path);
        relay.post(
            "/",
            Some("secret"),
            r#"{"merge_variables":{"data":{"bar":null}}}"#,
        );
        // give the write a moment to hit disk before we kill the process
        std::thread::sleep(Duration::from_millis(100));
    }

    let relay = start_relay(Some("secret"), false, &state_path);
    let (status, body) = relay.get("/");
    assert_eq!(status, 200);
    assert_eq!(body, r#"{"data":{"bar":null}}"#);
}

#[test]
fn poll_is_blocked_when_ip_allowlist_is_enabled_and_caller_is_not_localhost() {
    // With the allowlist enabled but no network access to fetch TRMNL's real IP list, the
    // allowlist falls back to localhost-only - so a request that spoofs a non-localhost
    // X-Forwarded-For should still be rejected, proving the header is actually consulted
    // rather than the check being a no-op.
    let dir = tempfile::tempdir().unwrap();
    let relay = start_relay(Some("secret"), true, &dir.path().join("state.json"));
    // give the background IP-list fetch a moment to fail over to the localhost fallback
    std::thread::sleep(Duration::from_millis(200));

    let (status, _) = status_and_body!(
        ureq::get(format!("{}/", relay.base_url))
            .header("X-Forwarded-For", "203.0.113.5")
            .call()
    );
    assert_eq!(status, 403);
}
