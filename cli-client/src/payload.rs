//! TRMNL webhook payload

use crate::Args;
use crate::sbuffer::SBuffer;
use serde::Serialize;
use serde_json::json;

#[inline]
pub fn make(args: &Args, snapshot: SBuffer) -> WebhookPayload {
    WebhookPayload {
        data: WebhookPayloadData::new(args, snapshot),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    data: WebhookPayloadData,
}

impl WebhookPayload {
    pub fn into_webhook_parts(self) -> (serde_json::Value, serde_json::Value) {
        let metadata_payload = json!({
            "merge_variables": {
                "data": {
                    "width": self.data.width,
                    "scale": self.data.scale,
                    "bar": self.data.bar
                }
            },
            "merge_strategy": "deep_merge"
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
    width: u16,
    scale: u8,
    bar: Option<WebhookPayloadBar>,
    content: String,
}

impl WebhookPayloadData {
    pub fn new(args: &Args, snapshot: SBuffer) -> Self {
        Self {
            width: args.width,
            scale: args.scale,
            bar: WebhookPayloadBar::new(&args),
            content: snapshot.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayloadBar {
    pub left: Option<String>,
    pub right: Option<String>,
    pub icon: Option<String>,
}

impl WebhookPayloadBar {
    pub fn new(args: &Args) -> Option<Self> {
        if args.bar_left.is_none() && args.bar_right.is_none() && args.bar_icon.is_none() {
            None
        } else {
            Some(Self {
                left: args.bar_left.clone(),
                right: args.bar_right.clone(),
                icon: args.bar_icon.clone(),
            })
        }
    }
}
