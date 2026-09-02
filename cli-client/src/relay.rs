//! Core logic for `trmnl-console-backend` (see `src/bin/trmnl-console-backend.rs`): a
//! self-hosted stand-in for TRMNL's own private-plugin webhook endpoint.
//!
//! It mirrors that endpoint's contract exactly (see
//! <https://docs.trmnl.com/go/private-plugins/webhooks>): one URL per id, `POST` pushes a
//! `{"merge_variables": {...}, "merge_strategy": ...}` payload (same wire format
//! `trmnl-console` already sends), `GET` on that *same* URL reads the current merged state
//! back. Point a "polling" plugin recipe's `polling_url` at the same URL you push to, and
//! it works exactly like a normal TRMNL webhook plugin would, just without TRMNL's webhook
//! rate/size limits on the push side. See `backend/README.md`.

use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
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

/// Only allow id path segments that are safe to use as a filename (also happens to match
/// the shape of a real webhook UUID) - rejects anything that could be a path-traversal
/// attempt (`..`, `/`) or is empty/absurdly long.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Holds one merged state per id, persisted as `<dir>/<id>.json` so a restart doesn't lose
/// the last-pushed content until the next push arrives. Each id behaves like an independent
/// TRMNL webhook endpoint - unrelated ids never see each other's state.
#[derive(Clone)]
pub struct MultiStore {
    dir: Option<PathBuf>,
    states: Arc<RwLock<HashMap<String, Value>>>,
}

impl MultiStore {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            states: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn path_for(&self, id: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| dir.join(format!("{id}.json")))
    }

    /// Returns the current state for `id`, loading it from disk on first access if not
    /// already cached in memory. `{}` if nothing has ever been pushed for this id.
    pub async fn get(&self, id: &str) -> Value {
        if let Some(state) = self.states.read().await.get(id) {
            return state.clone();
        }
        let loaded = match self.path_for(id) {
            Some(path) => tokio::fs::read(&path)
                .await
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_else(|| Value::Object(Default::default())),
            None => Value::Object(Default::default()),
        };
        self.states
            .write()
            .await
            .insert(id.to_string(), loaded.clone());
        loaded
    }

    /// Merges `incoming` into `id`'s stored state per `strategy` and persists the result
    /// (if a directory was configured).
    pub async fn merge(
        &self,
        id: &str,
        incoming: Value,
        strategy: MergeStrategy,
    ) -> std::io::Result<()> {
        let mut current = self.get(id).await;
        apply_merge(&mut current, incoming, strategy);

        if let Some(path) = self.path_for(id) {
            let bytes = serde_json::to_vec_pretty(&current)?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(path, bytes).await?;
        }

        self.states.write().await.insert(id.to_string(), current);
        Ok(())
    }
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

    #[test]
    fn id_validation_rejects_traversal_and_empty() {
        assert!(is_valid_id("a1b2c3d4-e5f6-4a5b-8c9d-0123456789ab"));
        assert!(is_valid_id("simple_id-123"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("../etc/passwd"));
        assert!(!is_valid_id("has/slash"));
        assert!(!is_valid_id(&"x".repeat(129)));
    }

    #[tokio::test]
    async fn different_ids_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let store = MultiStore::new(Some(dir.path().to_path_buf()));

        store
            .merge("a", json!({"data": {"bar": "a"}}), MergeStrategy::Replace)
            .await
            .unwrap();
        store
            .merge("b", json!({"data": {"bar": "b"}}), MergeStrategy::Replace)
            .await
            .unwrap();

        assert_eq!(store.get("a").await, json!({"data": {"bar": "a"}}));
        assert_eq!(store.get("b").await, json!({"data": {"bar": "b"}}));
    }

    #[tokio::test]
    async fn state_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let store = MultiStore::new(Some(path.clone()));
        store
            .merge(
                "my-id",
                json!({"data": {"bar": null}}),
                MergeStrategy::Replace,
            )
            .await
            .unwrap();

        // Fresh store (simulating a restart), same directory - must load from disk, not
        // from the previous instance's in-memory cache.
        let reloaded = MultiStore::new(Some(path));
        assert_eq!(reloaded.get("my-id").await, json!({"data": {"bar": null}}));
    }

    #[tokio::test]
    async fn unknown_id_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = MultiStore::new(Some(dir.path().to_path_buf()));
        assert_eq!(store.get("never-pushed").await, json!({}));
    }
}
