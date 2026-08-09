use crate::payload::WebhookPayload;
use owo_colors::{OwoColorize, Stream::Stderr};
use reqwest::{Client, Error, StatusCode};
use serde_json::Value;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub struct SendWebhookError {
    #[allow(unused)] // may use this in the future
    pub was_metadata_sent: bool,
    pub kind: SendWebhookErrorKind,
}

pub enum SendWebhookErrorKind {
    Request(Error),
    RequestVolumeLimitReached(String),
    RequestSizeLimitReached(String),
    ServerError(StatusCode, String),
    ClientError(StatusCode, String),
    UnknownError(StatusCode, String),
}

pub async fn send_cli(url: String, payload: WebhookPayload) -> u8 {
    match send(url, payload).await {
        Ok(_) => 0,
        Err(err) => {
            let err_prefix = "trmnl-console: failed updating plugin:\n"
                .if_supports_color(Stderr, |text| text.red());
            match err.kind {
                SendWebhookErrorKind::Request(err) => {
                    eprintln!(
                        "{}request failure. Did you enter the correct URL?\nDetails:\n{}",
                        err_prefix, err
                    )
                }
                SendWebhookErrorKind::RequestVolumeLimitReached(err) => {
                    eprintln!(
                        "{}reached a limit. You may have sent too many webhooks today.\nSee https://docs.trmnl.com/go/private-plugins/webhooks for limits.\nDetails:\n{}",
                        err_prefix,
                        format_err_details(err)
                    )
                }
                SendWebhookErrorKind::RequestSizeLimitReached(err) => {
                    eprintln!(
                        "{}reached a limit. Your console output may be too large.\nSee https://docs.trmnl.com/go/private-plugins/webhooks for limits.\nDetails:\n{}",
                        err_prefix,
                        format_err_details(err)
                    )
                }
                SendWebhookErrorKind::ServerError(status, err) => {
                    eprintln!(
                        "{}server error ({}).\nDetails:\n{}",
                        err_prefix,
                        status,
                        format_err_details(err)
                    )
                }
                SendWebhookErrorKind::ClientError(status, err) => {
                    eprintln!(
                        "{}client error ({}).\nDetails:\n{}",
                        err_prefix,
                        status,
                        format_err_details(err)
                    )
                }
                SendWebhookErrorKind::UnknownError(status, err) => {
                    eprintln!(
                        "{}error ({}).\nDetails:\n{}",
                        err_prefix,
                        status,
                        format_err_details(err)
                    )
                }
            };
            90
        }
    }
}

fn format_err_details(err: String) -> String {
    if err.is_empty() {
        return "not available".to_string();
    }
    if let Ok(json) = serde_json::from_str::<Value>(&err) {
        if let Value::Object(obj) = json {
            if let Some(message) = obj.get("message") {
                if let Value::String(message) = message {
                    return message.clone();
                } else {
                    return message.to_string();
                }
            }
        }
    }
    err
}

pub async fn send(url: String, payload: WebhookPayload) -> Result<(), SendWebhookError> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .unwrap();

    let (metadata_payload, content_payload) = payload.into_webhook_parts();

    if let Err(err) = send_single_payload(&client, &url, metadata_payload).await {
        return Err(SendWebhookError {
            was_metadata_sent: false,
            kind: err,
        });
    }
    if let Err(err) = send_single_payload(&client, &url, content_payload).await {
        return Err(SendWebhookError {
            was_metadata_sent: true,
            kind: err,
        });
    }
    Ok(())
}

async fn send_single_payload(
    client: &Client,
    url: &String,
    payload: Value,
) -> Result<(), SendWebhookErrorKind> {
    match client.post(url).json(&payload).send().await {
        Ok(response) => {
            if response.status().is_success() {
                Ok(())
            } else if response.status() == StatusCode::TOO_MANY_REQUESTS {
                Err(SendWebhookErrorKind::RequestVolumeLimitReached(
                    response.text().await.unwrap_or_default(),
                ))
            } else if response.status() == StatusCode::UNPROCESSABLE_ENTITY {
                Err(SendWebhookErrorKind::RequestSizeLimitReached(
                    response.text().await.unwrap_or_default(),
                ))
            } else if response.status().is_server_error() {
                Err(SendWebhookErrorKind::ServerError(
                    response.status(),
                    response.text().await.unwrap_or_default(),
                ))
            } else if response.status().is_client_error() {
                Err(SendWebhookErrorKind::ClientError(
                    response.status(),
                    response.text().await.unwrap_or_default(),
                ))
            } else {
                Err(SendWebhookErrorKind::UnknownError(
                    response.status(),
                    response.text().await.unwrap_or_default(),
                ))
            }
        }
        Err(err) => Err(SendWebhookErrorKind::Request(err)),
    }
}
