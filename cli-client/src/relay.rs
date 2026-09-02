//! Core logic for `trmnl-console-relay`: an always-on HTTP service that accepts a
//! TRMNL webhook-shaped payload (`{"merge_variables": ..., "merge_strategy": ...}`, same
//! wire format `trmnl-console`/`trmnl-console-backend` already send) and serves the merged
//! result back to a TRMNL "polling" private plugin, instead of every push needing to go
//! directly to TRMNL's own rate/size-limited webhook endpoint. See `relay/README.md` for
//! the deployment story and `plugin/src/settings.yml` for wiring a plugin recipe up to a
//! running relay.
//!
//! This module holds the transport-independent pieces (merge semantics, persisted state,
//! the TRMNL IP allowlist); `src/bin/trmnl-console-relay.rs` wires them to HTTP routes.

use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// How an incoming payload's `merge_variables` combines with the currently stored state,
/// mirroring TRMNL's own webhook semantics - see
/// <https://docs.trmnl.com/go/private-plugins/webhooks>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Default when `merge_strategy` is omitted: the incoming `merge_variables` object
    /// entirely replaces the stored state.
    Replace,
    /// Recursively merges the incoming object into the stored state, key by key.
    DeepMerge,
    /// Any top-level array in the incoming object is appended to the same-named array in
    /// the stored state, then trimmed to the last `limit` entries. Non-array top-level
    /// keys are replaced, same as `Replace`.
    Stream { limit: usize },
}

impl MergeStrategy {
    pub fn from_request(
        merge_strategy: Option<&str>,
        stream_limit: Option<u64>,
    ) -> Result<Self, String> {
        match merge_strategy {
            None | Some("replace") => Ok(MergeStrategy::Replace),
            Some("deep_merge") => Ok(MergeStrategy::DeepMerge),
            Some("stream") => Ok(MergeStrategy::Stream {
                limit: stream_limit.unwrap_or(10) as usize,
            }),
            Some(other) => Err(format!("unknown merge_strategy '{other}'")),
        }
    }
}

/// Applies `incoming` onto `state` per `strategy`, in place.
pub fn apply_merge(state: &mut Value, incoming: Value, strategy: MergeStrategy) {
    match strategy {
        MergeStrategy::Replace => *state = incoming,
        MergeStrategy::DeepMerge => deep_merge(state, incoming),
        MergeStrategy::Stream { limit } => stream_merge(state, incoming, limit),
    }
}

fn deep_merge(state: &mut Value, incoming: Value) {
    match (state, incoming) {
        (Value::Object(state_map), Value::Object(incoming_map)) => {
            for (key, incoming_value) in incoming_map {
                match state_map.get_mut(&key) {
                    Some(existing) => deep_merge(existing, incoming_value),
                    None => {
                        state_map.insert(key, incoming_value);
                    }
                }
            }
        }
        (state_slot, incoming_value) => *state_slot = incoming_value,
    }
}

fn stream_merge(state: &mut Value, incoming: Value, limit: usize) {
    let Value::Object(incoming_map) = incoming else {
        *state = incoming;
        return;
    };
    if !state.is_object() {
        *state = Value::Object(Default::default());
    }
    let state_map = state
        .as_object_mut()
        .expect("just ensured state is an object");
    for (key, incoming_value) in incoming_map {
        if let Value::Array(incoming_items) = incoming_value {
            let entry = state_map
                .entry(key)
                .or_insert_with(|| Value::Array(Vec::new()));
            if !entry.is_array() {
                *entry = Value::Array(Vec::new());
            }
            let items = entry
                .as_array_mut()
                .expect("just ensured entry is an array");
            items.extend(incoming_items);
            if items.len() > limit {
                let excess = items.len() - limit;
                items.drain(0..excess);
            }
        } else {
            state_map.insert(key, incoming_value);
        }
    }
}

/// Holds the current merged state in memory, optionally persisted to a JSON file on disk
/// so a restart doesn't lose the last-pushed content until the next push arrives.
#[derive(Clone)]
pub struct Store {
    path: Option<PathBuf>,
    state: Arc<RwLock<Value>>,
}

impl Store {
    /// Loads the store, seeding it from `path` if it exists and parses. Starts empty
    /// (`{}`) otherwise - not an error, since a fresh relay has nothing pushed yet.
    pub async fn load(path: Option<PathBuf>) -> Self {
        let initial = match &path {
            Some(path) => tokio::fs::read(path)
                .await
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_else(|| Value::Object(Default::default())),
            None => Value::Object(Default::default()),
        };
        Self {
            path,
            state: Arc::new(RwLock::new(initial)),
        }
    }

    pub async fn get(&self) -> Value {
        self.state.read().await.clone()
    }

    /// Merges `incoming` into the stored state per `strategy` and persists the result (if
    /// a path was configured).
    pub async fn merge(&self, incoming: Value, strategy: MergeStrategy) -> std::io::Result<()> {
        let mut state = self.state.write().await;
        apply_merge(&mut state, incoming, strategy);
        if let Some(path) = &self.path {
            let bytes = serde_json::to_vec_pretty(&*state)?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(path, bytes).await?;
        }
        Ok(())
    }
}

const TRMNL_IPS_API: &str = "https://trmnl.com/api/ips";

#[derive(serde::Deserialize)]
struct IpsResponse {
    data: IpsData,
}

#[derive(serde::Deserialize, Default)]
struct IpsData {
    #[serde(default)]
    ipv4: Vec<String>,
    #[serde(default)]
    ipv6: Vec<String>,
}

/// Restricts the polling endpoint to TRMNL's published server IPs, refreshed
/// periodically - the pattern documented for protecting polling endpoints (TRMNL sends no
/// auth of its own on polling requests). Always includes localhost, for local testing.
#[derive(Clone)]
pub struct IpAllowlist {
    ips: Arc<RwLock<HashSet<String>>>,
}

impl IpAllowlist {
    fn localhost() -> HashSet<String> {
        ["127.0.0.1".to_string(), "::1".to_string()]
            .into_iter()
            .collect()
    }

    async fn fetch() -> Option<HashSet<String>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        let resp = client.get(TRMNL_IPS_API).send().await.ok()?;
        let parsed: IpsResponse = resp.json().await.ok()?;
        let mut set = Self::localhost();
        set.extend(parsed.data.ipv4);
        set.extend(parsed.data.ipv6);
        Some(set)
    }

    /// Does an initial fetch (falling back to localhost-only if it fails) and spawns a
    /// background refresh loop. A failed refresh never clobbers a working allowlist with
    /// an empty one - it just keeps the last known-good set until the next attempt.
    pub async fn start(refresh_hours: u64) -> Self {
        let initial = Self::fetch().await.unwrap_or_else(Self::localhost);
        let slf = Self {
            ips: Arc::new(RwLock::new(initial)),
        };
        let ips = slf.ips.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(refresh_hours.max(1) * 3600)).await;
                if let Some(fresh) = Self::fetch().await {
                    *ips.write().await = fresh;
                }
            }
        });
        slf
    }

    pub async fn contains(&self, ip: &str) -> bool {
        self.ips.read().await.contains(ip)
    }
}

/// Picks the real client IP out of proxy headers (Cloudflare/X-Forwarded-For/X-Real-IP, in
/// that priority order - same as the documented `ip_whitelist.py` pattern), falling back to
/// the direct TCP peer address. `header` should return the first value of the given header
/// name, lowercased, if present.
pub fn client_ip(
    mut header: impl FnMut(&str) -> Option<String>,
    peer: Option<std::net::IpAddr>,
) -> Option<String> {
    for name in ["cf-connecting-ip", "x-forwarded-for", "x-real-ip"] {
        if let Some(value) = header(name) {
            let ip = value.split(',').next().unwrap_or("").trim();
            if !ip.is_empty() {
                return Some(ip.to_string());
            }
        }
    }
    peer.map(|ip| ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn replace_overwrites_whole_state() {
        let mut state = json!({"data": {"old": 1}});
        apply_merge(
            &mut state,
            json!({"data": {"new": 2}}),
            MergeStrategy::Replace,
        );
        assert_eq!(state, json!({"data": {"new": 2}}));
    }

    #[test]
    fn deep_merge_combines_nested_objects() {
        let mut state = json!({"data": {"bar": {"left": "old"}, "keep": true}});
        apply_merge(
            &mut state,
            json!({"data": {"bar": {"left": "new"}}}),
            MergeStrategy::DeepMerge,
        );
        assert_eq!(
            state,
            json!({"data": {"bar": {"left": "new"}, "keep": true}})
        );
    }

    #[test]
    fn deep_merge_replaces_non_object_leaves() {
        let mut state = json!({"count": 1});
        apply_merge(&mut state, json!({"count": 2}), MergeStrategy::DeepMerge);
        assert_eq!(state, json!({"count": 2}));
    }

    #[test]
    fn stream_appends_and_trims_arrays() {
        let mut state = json!({"temperatures": [1, 2, 3]});
        apply_merge(
            &mut state,
            json!({"temperatures": [4, 5]}),
            MergeStrategy::Stream { limit: 4 },
        );
        assert_eq!(state, json!({"temperatures": [2, 3, 4, 5]}));
    }

    #[test]
    fn stream_replaces_non_array_keys() {
        let mut state = json!({"label": "old"});
        apply_merge(
            &mut state,
            json!({"label": "new"}),
            MergeStrategy::Stream { limit: 10 },
        );
        assert_eq!(state, json!({"label": "new"}));
    }

    #[test]
    fn merge_strategy_parses_known_values() {
        assert_eq!(
            MergeStrategy::from_request(None, None).unwrap(),
            MergeStrategy::Replace
        );
        assert_eq!(
            MergeStrategy::from_request(Some("replace"), None).unwrap(),
            MergeStrategy::Replace
        );
        assert_eq!(
            MergeStrategy::from_request(Some("deep_merge"), None).unwrap(),
            MergeStrategy::DeepMerge
        );
        assert_eq!(
            MergeStrategy::from_request(Some("stream"), Some(5)).unwrap(),
            MergeStrategy::Stream { limit: 5 }
        );
        assert_eq!(
            MergeStrategy::from_request(Some("stream"), None).unwrap(),
            MergeStrategy::Stream { limit: 10 }
        );
        assert!(MergeStrategy::from_request(Some("bogus"), None).is_err());
    }

    #[tokio::test]
    async fn store_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");

        let store = Store::load(Some(path.clone())).await;
        store
            .merge(json!({"data": {"bar": null}}), MergeStrategy::Replace)
            .await
            .unwrap();
        assert_eq!(store.get().await, json!({"data": {"bar": null}}));

        let reloaded = Store::load(Some(path)).await;
        assert_eq!(reloaded.get().await, json!({"data": {"bar": null}}));
    }

    #[tokio::test]
    async fn store_starts_empty_without_a_prior_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let store = Store::load(Some(path)).await;
        assert_eq!(store.get().await, json!({}));
    }

    #[test]
    fn client_ip_prefers_cloudflare_header() {
        let ip = client_ip(
            |name| match name {
                "cf-connecting-ip" => Some("1.1.1.1".to_string()),
                "x-forwarded-for" => Some("2.2.2.2".to_string()),
                _ => None,
            },
            Some("3.3.3.3".parse().unwrap()),
        );
        assert_eq!(ip.as_deref(), Some("1.1.1.1"));
    }

    #[test]
    fn client_ip_falls_back_through_headers_then_peer() {
        assert_eq!(
            client_ip(|_| None, Some("9.9.9.9".parse().unwrap())).as_deref(),
            Some("9.9.9.9")
        );
        assert_eq!(
            client_ip(
                |name| (name == "x-forwarded-for").then(|| "8.8.8.8, 1.2.3.4".to_string()),
                None
            )
            .as_deref(),
            Some("8.8.8.8")
        );
    }
}
