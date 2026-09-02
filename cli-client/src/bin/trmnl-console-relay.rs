//! `trmnl-console-relay`: an always-on HTTP service that accepts a TRMNL webhook-shaped
//! payload and serves it back to a TRMNL "polling" private plugin.
//!
//! Point `trmnl-console`/`trmnl-console-backend`'s `--url`/`webhook_url` at this relay
//! instead of TRMNL directly - they already speak the wire format this expects
//! (`{"merge_variables": {...}, "merge_strategy": ...}`), so no changes needed on that
//! side. Then set the plugin recipe's `strategy: polling` with `polling_url` pointing back
//! at this relay (see plugin/src/settings.yml and relay/README.md) so TRMNL fetches from
//! here on its own schedule, decoupled from the sender and from TRMNL's webhook rate/size
//! limits.
//!
//! Config is via environment variables (see relay/README.md for the full list):
//! `RELAY_BIND`, `RELAY_TOKEN`, `RELAY_STATE_PATH`, `RELAY_IP_ALLOWLIST`,
//! `RELAY_IP_REFRESH_HOURS`.

use serde_json::Value;
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use trmnl_console::relay::{IpAllowlist, MergeStrategy, Store, client_ip};
use warp::Filter;
use warp::http::StatusCode;

#[tokio::main]
async fn main() {
    let bind: SocketAddr = env::var("RELAY_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()
        .expect("RELAY_BIND must be a valid <ip>:<port>");

    let token = env::var("RELAY_TOKEN").ok();
    if token.is_none() {
        eprintln!(
            "trmnl-console-relay: RELAY_TOKEN is not set - every push will be rejected. Set it to a random secret and configure the same value as an Authorization: Bearer header on the sending side."
        );
    }

    let state_path = env::var("RELAY_STATE_PATH")
        .ok()
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("/data/state.json")));
    let store = Store::load(state_path).await;

    let ip_allowlist_enabled = env::var("RELAY_IP_ALLOWLIST")
        .map(|v| v.to_lowercase() != "false")
        .unwrap_or(true);
    let refresh_hours: u64 = env::var("RELAY_IP_REFRESH_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24);
    let allowlist = if ip_allowlist_enabled {
        eprintln!("trmnl-console-relay: fetching TRMNL's IP allowlist for the polling endpoint...");
        Some(IpAllowlist::start(refresh_hours).await)
    } else {
        eprintln!(
            "trmnl-console-relay: RELAY_IP_ALLOWLIST=false - the polling endpoint is open to anyone who can reach it."
        );
        None
    };

    let health = warp::path("health").and(warp::get()).map(|| "ok");

    let post_store = store.clone();
    let post_token = token.clone();
    let post = warp::path::end()
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(1024 * 1024))
        .and(warp::body::json())
        .and_then(move |auth: Option<String>, body: Value| {
            let store = post_store.clone();
            let token = post_token.clone();
            async move { handle_post(store, token, auth, body).await }
        });

    let get_store = store.clone();
    let get = warp::path::end()
        .and(warp::get())
        .and(warp::header::headers_cloned())
        .and(warp::filters::addr::remote())
        .and_then(
            move |headers: warp::http::HeaderMap, remote: Option<SocketAddr>| {
                let store = get_store.clone();
                let allowlist = allowlist.clone();
                async move { handle_get(store, allowlist, headers, remote).await }
            },
        );

    let routes = health.or(post).or(get);

    eprintln!("trmnl-console-relay: listening on http://{bind}");
    warp::serve(routes).run(bind).await;
}

async fn handle_post(
    store: Store,
    token: Option<String>,
    auth_header: Option<String>,
    body: Value,
) -> Result<Box<dyn warp::Reply>, std::convert::Infallible> {
    let Some(expected) = token else {
        return Ok(Box::new(warp::reply::with_status(
            "relay has no RELAY_TOKEN configured, refusing all pushes",
            StatusCode::SERVICE_UNAVAILABLE,
        )));
    };
    let provided = auth_header
        .as_deref()
        .and_then(|value| value.strip_prefix("Bearer "));
    if provided != Some(expected.as_str()) {
        return Ok(Box::new(warp::reply::with_status(
            "missing or incorrect Authorization: Bearer token",
            StatusCode::UNAUTHORIZED,
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

    match store.merge(merge_variables, strategy).await {
        Ok(()) => Ok(Box::new(warp::reply::json(&serde_json::json!({})))),
        Err(err) => {
            eprintln!("trmnl-console-relay: failed to persist state: {err}");
            Ok(Box::new(warp::reply::with_status(
                "failed to persist state",
                StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
    }
}

async fn handle_get(
    store: Store,
    allowlist: Option<IpAllowlist>,
    headers: warp::http::HeaderMap,
    remote: Option<SocketAddr>,
) -> Result<Box<dyn warp::Reply>, std::convert::Infallible> {
    if let Some(allowlist) = allowlist {
        let ip = client_ip(
            |name| {
                headers
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.to_string())
            },
            remote.map(|addr| addr.ip()),
        );
        let allowed = match &ip {
            Some(ip) => allowlist.contains(ip).await,
            None => false,
        };
        if !allowed {
            eprintln!(
                "trmnl-console-relay: blocked poll from {} (not in TRMNL's IP allowlist)",
                ip.as_deref().unwrap_or("<unknown>")
            );
            return Ok(Box::new(warp::reply::with_status(
                "not in TRMNL's published IP range",
                StatusCode::FORBIDDEN,
            )));
        }
    }

    Ok(Box::new(warp::reply::json(&store.get().await)))
}
