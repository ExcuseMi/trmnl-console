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
    LimitReached,
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
                        "{}request failure. Did you enter the correct URL? Details: {}",
                        err_prefix, err
                    )
                }
                SendWebhookErrorKind::LimitReached => {
                    eprintln!(
                        "{}reached a limit. Your console output may be too large or you have sent too many webhooks today.\n See https://docs.trmnl.com/go/private-plugins/webhooks for limits.",
                        err_prefix
                    )
                }
                SendWebhookErrorKind::ServerError(status, err) => {
                    eprintln!("{}server error ({}). Details: {}", err_prefix, status, err)
                }
                SendWebhookErrorKind::ClientError(status, err) => {
                    eprintln!("{}client error ({}). Details: {}", err_prefix, status, err)
                }
                SendWebhookErrorKind::UnknownError(status, err) => {
                    eprintln!("{}error ({}). Details: {}", err_prefix, status, err)
                }
            };
            90
        }
    }
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
                Err(SendWebhookErrorKind::LimitReached)
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
