//! `trmnl-console-backend`: runs one or more scheduled capture jobs and pushes their
//! output to a TRMNL private plugin webhook, using the same TRMNL webhook-style payload
//! (`merge_variables`, see <https://docs.trmnl.com/go/private-plugins/webhooks>) as the
//! `trmnl-console` CLI's `--url` mode.
//!
//! Unlike the CLI, a job is not told an exact device size up front: it captures the same
//! raw command output once per tick and replays it into several configured terminal
//! sizes, sending all of them as "variants" in a single payload. The plugin recipe
//! (`plugin/src/shared.liquid`) picks whichever variant best fits the screen space it is
//! actually given at render time. See `backend/README.md` for job config examples,
//! including tmux and Zellij session capture.

use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::ExitCode;
use std::time::Duration;
use tokio::time::MissedTickBehavior;
use trmnl_console::payload::{WebhookPayloadBar, WebhookPayloadDataMulti, WebhookPayloadVariant};
use trmnl_console::virtual_terminal::VirtualTerminal;
use trmnl_console::webhook;

#[derive(Debug, clap::Parser)]
#[command(
    version,
    about = "Runs scheduled trmnl-console capture jobs and pushes them to a TRMNL webhook."
)]
struct CliArgs {
    /// Path to the backend job config file (YAML). See backend/README.md for the format.
    #[arg(
        short,
        long,
        env = "TRMNL_CONSOLE_BACKEND_CONFIG",
        default_value = "backend.yml"
    )]
    config: PathBuf,
    /// Run every configured job once immediately, then exit. Useful to sanity-check a
    /// config and see what gets sent before leaving it running on a schedule.
    #[arg(long)]
    once: bool,
}

#[derive(Debug, Deserialize)]
struct Config {
    /// Default webhook URL for jobs that don't set their own `webhook_url`.
    #[serde(default)]
    webhook_url: Option<String>,
    /// Default bearer token for jobs that don't set their own `webhook_token`. Only needed
    /// when `webhook_url` is a self-hosted `trmnl-console-relay`, not TRMNL's own webhook
    /// endpoint (whose URL already embeds its own secret).
    #[serde(default)]
    webhook_token: Option<String>,
    #[serde(default)]
    jobs: Vec<JobConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct JobConfig {
    /// Used only for logging.
    name: String,
    /// Shell command (run via `sh -c`) whose stdout is captured once per tick and then
    /// replayed into every size in `sizes`. Use this to dump a tmux/Zellij pane, or run
    /// any other one-shot command that produces terminal-formatted output on its own
    /// (i.e. not an interactive TUI that needs to be driven live in a PTY - see
    /// backend/README.md for that case).
    command: String,
    /// Seconds between runs.
    interval_seconds: u64,
    /// Overrides the top-level `webhook_url` for this job.
    #[serde(default)]
    webhook_url: Option<String>,
    /// Overrides the top-level `webhook_token` for this job.
    #[serde(default)]
    webhook_token: Option<String>,
    /// Also feed the command's stderr into the captured terminal, like the CLI's
    /// `--pass-stderr`.
    #[serde(default)]
    pass_stderr: bool,
    /// How long to let each size's virtual terminal settle after the captured bytes have
    /// all been fed in, before taking the snapshot.
    #[serde(default = "default_wait_time_seconds")]
    wait_time_seconds: f32,
    #[serde(default)]
    bar: Option<BarConfig>,
    /// Sizes to capture and send as variants. Defaults to the full device table from the
    /// README (`default_sizes` below) if omitted. Sending many variants is more likely to
    /// exceed TRMNL's webhook payload size limit (2kb, 5kb for TRMNL+) for verbose
    /// output - narrow this down to the device(s) you actually own for anything but
    /// sparse output.
    #[serde(default = "default_sizes")]
    sizes: Vec<SizeConfig>,
    /// If the encoded payload is still over this many bytes after dropping the largest
    /// size variants to try to fit, it is sent anyway (TRMNL will reject it with a clear
    /// error). Defaults to TRMNL's free-tier webhook limit; raise it if you're on
    /// TRMNL+ (5000).
    #[serde(default = "default_max_payload_bytes")]
    max_payload_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct BarConfig {
    #[serde(default)]
    left: Option<String>,
    #[serde(default)]
    right: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SizeConfig {
    id: String,
    width: u16,
    height: u16,
    #[serde(default = "default_scale")]
    scale: u8,
}

fn default_scale() -> u8 {
    1
}

fn default_wait_time_seconds() -> f32 {
    2.0
}

fn default_max_payload_bytes() -> usize {
    2000
}

/// The standard device/orientation/scale table from the top-level README's "Devices"
/// section, used when a job does not list its own `sizes`.
fn default_sizes() -> Vec<SizeConfig> {
    [
        ("trmnl-og-landscape-x1", 111, 29, 1),
        ("trmnl-og-landscape-x2", 55, 14, 2),
        ("trmnl-og-landscape-x3", 37, 9, 3),
        ("trmnl-x-landscape-x1", 145, 51, 1),
        ("trmnl-x-landscape-x2", 73, 25, 2),
        ("trmnl-x-landscape-x3", 48, 17, 3),
        ("trmnl-x-landscape-x4", 36, 12, 4),
        ("trmnl-x-portrait-x1", 108, 69, 1),
        ("trmnl-x-portrait-x2", 54, 34, 2),
        ("trmnl-x-portrait-x3", 35, 23, 3),
    ]
    .into_iter()
    .map(|(id, width, height, scale)| SizeConfig {
        id: id.to_string(),
        width,
        height,
        scale,
    })
    .collect()
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Some(code) = trmnl_console::try_run_internal_subprocess().await {
        return code;
    }

    let args = CliArgs::parse();
    let config = match load_config(&args.config) {
        Ok(config) => config,
        Err(err) => {
            eprintln!(
                "trmnl-console-backend: failed to load config {}: {err}",
                args.config.display()
            );
            return ExitCode::from(91);
        }
    };
    if config.jobs.is_empty() {
        eprintln!("trmnl-console-backend: config has no jobs, nothing to do");
        return ExitCode::from(91);
    }

    let mut jobs = Vec::with_capacity(config.jobs.len());
    for job in config.jobs {
        let Some(url) = job
            .webhook_url
            .clone()
            .or_else(|| config.webhook_url.clone())
        else {
            eprintln!(
                "trmnl-console-backend: job '{}' has no webhook_url and no top-level default is set, skipping",
                job.name
            );
            continue;
        };
        let token = job
            .webhook_token
            .clone()
            .or_else(|| config.webhook_token.clone());
        if job.sizes.is_empty() {
            eprintln!(
                "trmnl-console-backend: job '{}' has an empty sizes list, skipping",
                job.name
            );
            continue;
        }
        jobs.push((job, Destination { url, token }));
    }
    if jobs.is_empty() {
        eprintln!("trmnl-console-backend: no runnable jobs after validation, exiting");
        return ExitCode::from(91);
    }

    if args.once {
        for (job, destination) in &jobs {
            run_job_once(job, destination).await;
        }
        return ExitCode::SUCCESS;
    }

    let handles: Vec<_> = jobs
        .into_iter()
        .map(|(job, destination)| tokio::spawn(run_job_loop(job, destination)))
        .collect();

    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for shutdown signal");
    eprintln!("trmnl-console-backend: shutting down...");
    for handle in handles {
        handle.abort();
    }

    ExitCode::SUCCESS
}

fn load_config(path: &PathBuf) -> Result<Config, String> {
    let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_yaml::from_str(&raw).map_err(|err| err.to_string())
}

/// Where a job's payload goes: TRMNL's own webhook endpoint (`token: None`, the URL's UUID
/// is the secret) or a self-hosted `trmnl-console-relay` (`token: Some(..)`, sent as
/// `Authorization: Bearer`).
struct Destination {
    url: String,
    token: Option<String>,
}

async fn run_job_loop(job: JobConfig, destination: Destination) {
    let mut interval = tokio::time::interval(Duration::from_secs(job.interval_seconds.max(1)));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        run_job_once(&job, &destination).await;
    }
}

async fn run_job_once(job: &JobConfig, destination: &Destination) {
    match run_job(job, destination).await {
        Ok(()) => println!(
            "trmnl-console-backend: job '{}' sent successfully",
            job.name
        ),
        Err(err) => eprintln!("trmnl-console-backend: job '{}' failed: {err}", job.name),
    }
}

async fn run_job(job: &JobConfig, destination: &Destination) -> Result<(), String> {
    let output = capture_command_output(&job.command)
        .await
        .map_err(|err| format!("command '{}' failed: {err}", job.command))?;

    let mut variants = Vec::with_capacity(job.sizes.len());
    for size in &job.sizes {
        let snapshot = capture_variant(&output, size, job.pass_stderr, job.wait_time_seconds)
            .await
            .map_err(|err| format!("capture for size '{}' failed: {err}", size.id))?;
        variants.push(WebhookPayloadVariant {
            id: size.id.clone(),
            width: size.width,
            scale: size.scale,
            content: snapshot.to_string(),
        });
    }

    let bar = job.bar.as_ref().and_then(|bar| {
        WebhookPayloadBar::new(bar.left.clone(), bar.right.clone(), bar.icon.clone())
    });

    let variants = trim_to_budget(&bar, variants, job.max_payload_bytes, &job.name);
    let payload = WebhookPayloadDataMulti { bar, variants }.into_webhook();

    webhook::send_one(
        destination.url.clone(),
        payload,
        destination.token.as_deref(),
    )
    .await
    .map_err(|err| webhook::describe_error(&err.kind))
}

/// Runs `command` through a shell once and returns its raw stdout bytes, to be replayed
/// into every configured size's virtual terminal. Not run per-size: a command like `tmux
/// capture-pane` already produces a fixed snapshot of pane content, so running it once and
/// reusing the bytes keeps all variants consistent with each other and avoids hitting a
/// live session N times per tick.
async fn capture_command_output(command: &str) -> Result<Vec<u8>, String> {
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .await
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

async fn capture_variant(
    bytes: &[u8],
    size: &SizeConfig,
    pass_stderr: bool,
    wait_time_seconds: f32,
) -> Result<trmnl_console::sbuffer::SBuffer, String> {
    let reader: Pin<Box<dyn tokio::io::AsyncRead + Send>> =
        Box::pin(std::io::Cursor::new(bytes.to_vec()));
    let term = VirtualTerminal::run_with_async_reader(size.width, size.height, pass_stderr, reader)
        .await
        .map_err(|err| err.to_string())?;
    let _ = tokio::time::timeout(
        Duration::from_secs_f32(wait_time_seconds),
        term.wait_for_finish(),
    )
    .await;
    match term.snapshot().await {
        Ok(Ok(snapshot)) => Ok(snapshot),
        Ok(Err(exit_code)) => Err(format!("virtual terminal exited with code {exit_code}")),
        Err(err) => Err(format!("{err:?}")),
    }
}

/// Drops the size variant contributing the most bytes, one at a time, until the encoded
/// payload fits within `max_bytes` or only one variant is left. This keeps as many size
/// options as the budget allows instead of failing outright or always sending everything
/// configured.
fn trim_to_budget(
    bar: &Option<WebhookPayloadBar>,
    mut variants: Vec<WebhookPayloadVariant>,
    max_bytes: usize,
    job_name: &str,
) -> Vec<WebhookPayloadVariant> {
    loop {
        let size = estimate_payload_bytes(bar, &variants);
        if size <= max_bytes || variants.len() <= 1 {
            if size > max_bytes {
                eprintln!(
                    "trmnl-console-backend: job '{job_name}': payload is {size} bytes (budget {max_bytes}) even with only one size variant left; sending anyway",
                );
            }
            return variants;
        }
        let (idx, _) = variants
            .iter()
            .enumerate()
            .max_by_key(|(_, variant)| variant.content.len())
            .expect("variants is non-empty");
        let dropped = variants.remove(idx);
        eprintln!(
            "trmnl-console-backend: job '{job_name}': payload was {size} bytes, over budget {max_bytes}; dropped size variant '{}'",
            dropped.id
        );
    }
}

fn estimate_payload_bytes(
    bar: &Option<WebhookPayloadBar>,
    variants: &[WebhookPayloadVariant],
) -> usize {
    let data = WebhookPayloadDataMulti {
        bar: bar.clone(),
        variants: variants.to_vec(),
    };
    serde_json::to_vec(&data.into_webhook())
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}
