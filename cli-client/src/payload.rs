//! TRMNL webhook payload

use crate::sbuffer::SBuffer;
use serde::Serialize;
use serde_json::json;

#[inline]
pub fn make(
    width: u16,
    scale: u8,
    bar: Option<WebhookPayloadBar>,
    snapshot: SBuffer,
) -> WebhookPayload {
    WebhookPayload {
        data: WebhookPayloadData {
            width,
            scale,
            bar,
            content: snapshot.to_string(),
        },
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    pub data: WebhookPayloadData,
}

impl WebhookPayload {
    #[allow(unused)]
    pub fn into_webhook(self) -> serde_json::Value {
        json!({
            "merge_variables": {
                "data": {
                    "width": self.data.width,
                    "scale": self.data.scale,
                    "bar": self.data.bar,
                    "content": self.data.content
                }
            }
        })
    }
    #[allow(unused)]
    pub fn into_webhook_parts(self) -> (serde_json::Value, serde_json::Value) {
        let metadata_payload = json!({
            "merge_variables": {
                "data": {
                    "width": self.data.width,
                    "scale": self.data.scale,
                    "bar": self.data.bar
                }
            }
        });
        let content_payload = json!({
            "merge_variables": {
                "data": {
                    "content": self.data.content,
                }
            },
            "merge_strategy": "deep_merge"
        });
        (metadata_payload, content_payload)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayloadData {
    pub width: u16,
    pub scale: u8,
    pub bar: Option<WebhookPayloadBar>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayloadBar {
    pub left: Option<String>,
    pub right: Option<String>,
    pub icon: Option<String>,
}

impl WebhookPayloadBar {
    pub fn new(left: Option<String>, right: Option<String>, icon: Option<String>) -> Option<Self> {
        if left.is_none() && right.is_none() && icon.is_none() {
            None
        } else {
            Some(Self { left, right, icon })
        }
    }
}

/// One rendering of a capture at a specific terminal size, as sent by
/// `trmnl-console-backend`. Several of these may be sent in a single webhook payload
/// (see [`WebhookPayloadDataMulti`]) so that the plugin can pick the one that best fits
/// whatever screen space it is actually given, without the sender having to know that
/// size in advance.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayloadVariant {
    /// Arbitrary identifier for this size, e.g. `"trmnl-og-landscape-x1"`. Only used for
    /// logging/debugging on the sending side; the plugin does not match on it.
    pub id: String,
    pub width: u16,
    pub scale: u8,
    pub content: String,
}

/// Webhook payload shape sent by `trmnl-console-backend`: a shared bottom bar plus a set
/// of size variants, as opposed to the single-size [`WebhookPayloadData`] the CLI sends
/// with `--url`. Both shapes are accepted by the same plugin recipe (see
/// `plugin/src/shared.liquid`).
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayloadDataMulti {
    pub bar: Option<WebhookPayloadBar>,
    pub variants: Vec<WebhookPayloadVariant>,
}

impl WebhookPayloadDataMulti {
    pub fn into_webhook(self) -> serde_json::Value {
        json!({
            "merge_variables": {
                "data": {
                    "bar": self.bar,
                    "variants": self.variants
                }
            }
        })
    }
}
