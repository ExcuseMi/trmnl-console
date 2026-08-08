//! --json output mode: the webhook payload's merge_variables content printed
//! to stdout. Structure per the "JSON Output" section of the main.rs docs.

mod common;

use common::{Cmd, PLAIN};
use serde_json::Value;

fn json_run(extra_args: &[&str]) -> Value {
    let out = Cmd::new()
        .size(PLAIN.cols, PLAIN.rows)
        .args(["--json"])
        .args(extra_args)
        .stdin_bytes(PLAIN.ansi)
        .run();
    assert_eq!(
        out.code(),
        0,
        "stdout: {:?}, stderr: {}",
        out.stdout,
        out.stderr
    );
    serde_json::from_str(&out.stdout).unwrap_or_else(|e| {
        panic!("stdout is not valid JSON ({e});\nstdout: {:?}", out.stdout)
    })
}

#[test]
fn json_structure_defaults() {
    let v = json_run(&[]);
    assert_eq!(v["data"]["width"], 4);
    assert_eq!(v["data"]["scale"], 1);
    assert!(
        v["data"]["bar"].is_null(),
        "bar must be JSON null without --bar-* options, got {}",
        v["data"]["bar"]
    );
    let content = v["data"]["content"]
        .as_str()
        .expect("data.content must be a string");
    assert!(
        content.starts_with('\u{E000}'),
        "SBuffer content must start with U+E000: {content:?}"
    );
}

#[test]
fn json_content_roundtrip() {
    // Pins the exact SBuffer encoder policy through the whole pipeline.
    let v = json_run(&[]);
    assert_eq!(v["data"]["content"], PLAIN.sbuffer);
}

#[test]
fn json_scale_passthrough() {
    let v = json_run(&["--scale", "3"]);
    assert_eq!(v["data"]["scale"], 3);
}

#[test]
fn json_bar_partial() {
    // Unset bar members are null, not missing ("string or null" per docs).
    let v = json_run(&["--bar-left", "L", "--bar-icon", "http://example.com/i.png"]);
    assert_eq!(
        v["data"]["bar"],
        serde_json::json!({"left": "L", "right": null, "icon": "http://example.com/i.png"})
    );
}

#[test]
fn json_bar_empty_string_still_enables_bar() {
    // "(Only) if any of the --bar-* arguments are provided, the title bar is
    // rendered" — an empty string is still provided, so bar must be an object.
    // (bar == null without any --bar-* flag is covered in
    // json_structure_defaults.)
    let v = json_run(&["--bar-left", ""]);
    assert!(
        v["data"]["bar"].is_object(),
        "bar should be an object, got {}",
        v["data"]["bar"]
    );
}

#[test]
fn json_top_level_shape() {
    // The payload printed by --json is the merge_variables *content*: a single
    // top-level "data" key. The {"merge_variables": ...} envelope is
    // webhook-only (see tests/webhook.rs and the body builders in common).
    let v = json_run(&[]);
    let keys: Vec<&String> = v.as_object().expect("top level is an object").keys().collect();
    assert_eq!(keys, ["data"], "unexpected top-level keys: {keys:?}");
}
