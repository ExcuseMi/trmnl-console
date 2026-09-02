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
    ClientBuild(Error),
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
            eprintln!("{}{}", err_prefix, describe_error(&err.kind));
            90
        }
    }
}

/// Renders a [`SendWebhookErrorKind`] as a human-readable message, without any of the
/// terminal color codes [`send_cli`] adds for the interactive CLI. Used by
/// `trmnl-console-backend` to log failures for a scheduled job.
pub fn describe_error(kind: &SendWebhookErrorKind) -> String {
    match kind {
        SendWebhookErrorKind::ClientBuild(err) => {
            format!("could not initialize the HTTP client.\nDetails:\n{}", err)
        }
        SendWebhookErrorKind::Request(err) => {
            format!(
                "request failure. Did you enter the correct URL?\nDetails:\n{}",
                err
            )
        }
        SendWebhookErrorKind::RequestVolumeLimitReached(err) => {
            format!(
                "reached a limit. You may have sent too many webhooks today.\nSee https://docs.trmnl.com/go/private-plugins/webhooks for limits.\nDetails:\n{}",
                format_err_details(err.clone())
            )
        }
        SendWebhookErrorKind::RequestSizeLimitReached(err) => {
            format!(
                "reached a limit. Your console output may be too large.\nSee https://docs.trmnl.com/go/private-plugins/webhooks for limits.\nDetails:\n{}",
                format_err_details(err.clone())
            )
        }
        SendWebhookErrorKind::ServerError(status, err) => {
            format!(
                "server error ({}).\nDetails:\n{}",
                status,
                format_err_details(err.clone())
            )
        }
        SendWebhookErrorKind::ClientError(status, err) => {
            format!(
                "client error ({}).\nDetails:\n{}",
                status,
                format_err_details(err.clone())
            )
        }
        SendWebhookErrorKind::UnknownError(status, err) => {
            format!(
                "error ({}).\nDetails:\n{}",
                status,
                format_err_details(err.clone())
            )
        }
    }
}

fn format_err_details(err: String) -> String {
    if err.is_empty() {
        return "not available".to_string();
    }
    if let Ok(json) = serde_json::from_str::<Value>(&err)
        && let Value::Object(obj) = json
        && let Some(message) = obj.get("message")
    {
        if let Value::String(message) = message {
            return message.clone();
        } else {
            return message.to_string();
        }
    }
    err
}

pub async fn send(url: String, payload: WebhookPayload) -> Result<(), SendWebhookError> {
    let client = build_client().map_err(|kind| SendWebhookError {
        was_metadata_sent: false,
        kind,
    })?;

    let (metadata_payload, content_payload) = payload.into_webhook_parts();

    if let Err(err) = send_json(&client, &url, metadata_payload, None).await {
        return Err(SendWebhookError {
            was_metadata_sent: false,
            kind: err,
        });
    }
    if let Err(err) = send_json(&client, &url, content_payload, None).await {
        return Err(SendWebhookError {
            was_metadata_sent: true,
            kind: err,
        });
    }

    Ok(())
}

/// Sends a single already-built webhook JSON body (e.g. from
/// [`crate::payload::WebhookPayloadDataMulti::into_webhook`]) in one request, without the
/// CLI's metadata/content split. Used by `trmnl-console-backend`, which sends one payload
/// per job tick.
///
/// `bearer_token`, if given, is sent as `Authorization: Bearer <token>` - TRMNL's own
/// webhook endpoint doesn't need this (the URL's UUID is the secret), but a self-hosted
/// `trmnl-console-relay` does.
pub async fn send_one(
    url: String,
    payload: Value,
    bearer_token: Option<&str>,
) -> Result<(), SendWebhookError> {
    let client = build_client().map_err(|kind| SendWebhookError {
        was_metadata_sent: false,
        kind,
    })?;
    send_json(&client, &url, payload, bearer_token)
        .await
        .map_err(|kind| SendWebhookError {
            was_metadata_sent: false,
            kind,
        })
}

fn build_client() -> Result<Client, SendWebhookErrorKind> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(SendWebhookErrorKind::ClientBuild)
}

async fn send_json(
    client: &Client,
    url: &String,
    payload: Value,
    bearer_token: Option<&str>,
) -> Result<(), SendWebhookErrorKind> {
    let mut request = client.post(url).json(&payload);
    if let Some(token) = bearer_token {
        request = request.bearer_auth(token);
    }
    match request.send().await {
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
