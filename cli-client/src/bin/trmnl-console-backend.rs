//! `trmnl-console-backend`: a self-hosted stand-in for TRMNL's own private-plugin webhook
//! endpoint.
//!
//! It mirrors that endpoint's contract exactly (see
//! <https://docs.trmnl.com/go/private-plugins/webhooks>): `POST /<id>` pushes a
//! `{"merge_variables": {...}, "merge_strategy": ...}` payload - the same wire format
//! `trmnl-console`/`trmnl-console-pusher` already send - and `GET /<id>` reads the current
//! merged state back. Configure a "polling" plugin recipe's `polling_url` to the *same*
//! URL you push to (see plugin/src/settings.yml and backend/README.md), and it behaves
//! exactly like a normal TRMNL webhook-backed plugin, just without TRMNL's webhook
//! rate/size limits on the push side. The `<id>` is the only credential - anyone who knows
//! it can push and poll, same as a real TRMNL webhook URL.
//!
//! Config is via environment variables (see backend/README.md for the full list):
//! `BACKEND_BIND`, `BACKEND_STATE_DIR`.

use serde_json::Value;
use std::env;
use std::path::PathBuf;
use trmnl_console::relay::{MergeStrategy, MultiStore, is_valid_id};
use warp::Filter;
use warp::http::StatusCode;

#[tokio::main]
async fn main() {
    let bind: std::net::SocketAddr = env::var("BACKEND_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()
        .expect("BACKEND_BIND must be a valid <ip>:<port>");

    let state_dir = env::var("BACKEND_STATE_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("/data")));
    let store = MultiStore::new(state_dir);

    let health = warp::path("health").and(warp::get()).map(|| "ok");

    let post_store = store.clone();
    let post = warp::path::param::<String>()
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::content_length_limit(1024 * 1024))
        .and(warp::body::json())
        .and_then(move |id: String, body: Value| {
            let store = post_store.clone();
            async move { handle_post(store, id, body).await }
        });

    let get_store = store.clone();
    let get = warp::path::param::<String>()
        .and(warp::path::end())
        .and(warp::get())
        .and_then(move |id: String| {
            let store = get_store.clone();
            async move { handle_get(store, id).await }
        });

    let routes = health.or(post).or(get);

    eprintln!("trmnl-console-backend: listening on http://{bind}");
    warp::serve(routes).run(bind).await;
}

async fn handle_post(
    store: MultiStore,
    id: String,
    body: Value,
) -> Result<Box<dyn warp::Reply>, std::convert::Infallible> {
    if !is_valid_id(&id) {
        return Ok(Box::new(warp::reply::with_status(
            "invalid id",
            StatusCode::NOT_FOUND,
        )));
    }

    let Some(merge_variables) = body.get("merge_variables").cloned() else {
        return Ok(Box::new(warp::reply::with_status(
            "body must be a JSON object with a \"merge_variables\" key",
            StatusCode::BAD_REQUEST,
        )));
    };
    let merge_strategy = body.get("merge_strategy").and_then(Value::as_str);
    let stream_limit = body.get("stream_limit").and_then(Value::as_u64);
    let strategy = match MergeStrategy::from_request(merge_strategy, stream_limit) {
        Ok(strategy) => strategy,
        Err(err) => {
            return Ok(Box::new(warp::reply::with_status(
                err,
                StatusCode::BAD_REQUEST,
            )));
        }
    };

    match store.merge(&id, merge_variables, strategy).await {
        Ok(()) => Ok(Box::new(warp::reply::json(&serde_json::json!({})))),
        Err(err) => {
            eprintln!("trmnl-console-backend: failed to persist state for '{id}': {err}");
            Ok(Box::new(warp::reply::with_status(
                "failed to persist state",
                StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
    }
}

async fn handle_get(
    store: MultiStore,
    id: String,
) -> Result<Box<dyn warp::Reply>, std::convert::Infallible> {
    if !is_valid_id(&id) {
        return Ok(Box::new(warp::reply::with_status(
            "invalid id",
            StatusCode::NOT_FOUND,
        )));
    }
    Ok(Box::new(warp::reply::json(&store.get(&id).await)))
}
