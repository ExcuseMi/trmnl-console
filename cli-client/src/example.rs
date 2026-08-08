use crate::sbuffer::SBuffer;
use shadow_terminal::Protocol;
use shadow_terminal::pty::{BytesFromPTY, BytesFromSTDIN};
use shadow_terminal::shadow_terminal::{Config, ShadowTerminal};
use std::time::Duration;

/// How long to wait for the command to produce its first output.
/// (TUI apps can be slow to paint their first frame — htop takes ~300ms.)
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
/// The screen counts as settled once the PTY has been quiet for this long.
const QUIET_PERIOD: Duration = Duration::from_millis(500);
/// Capture cutoff for commands that never stop redrawing.
const MAX_CAPTURE: Duration = Duration::from_secs(6);

pub async fn main() {
    // ShadowTerminal's own surface snapshots are unused; we read the wezterm
    // terminal directly.
    let (surface_tx, _surface_rx) = tokio::sync::mpsc::channel(1);
    let mut shadow = ShadowTerminal::new(
        Config {
            // The full-layout terminal grid on the device (see CLAUDE.md / shared.liquid).
            width: 111,
            height: 29,
            command: {
                let mut cmd: Vec<std::ffi::OsString> = vec![
                    "python".into(),
                    "/home/marco/dev/trmnl-console/colortest.py".into(),
                    "-s".into(),
                ];
                // Extra args are forwarded to colortest.py (e.g. `cargo run -- -o attrs`).
                cmd.extend(std::env::args_os().skip(1));
                cmd
            },
            scrollback_size: 0,
            scrollback_step: 0,
        },
        surface_tx,
    );
    let (pty_input_tx, pty_input_rx) = tokio::sync::mpsc::channel(2048);
    let _pty_task = shadow.start(pty_input_rx);
    let mut control_rx = shadow.channels.control_tx.subscribe();

    let started = tokio::time::Instant::now();
    let mut seen_output = false;
    loop {
        let idle_limit = if seen_output {
            QUIET_PERIOD
        } else {
            STARTUP_TIMEOUT
        };
        tokio::select! {
            received = tokio::time::timeout(idle_limit, shadow.channels.output_rx.recv()) => {
                match received {
                    Ok(Some(bytes)) => {
                        seen_output = true;
                        advance(&mut shadow, &pty_input_tx, &bytes).await;
                        if started.elapsed() > MAX_CAPTURE {
                            break;
                        }
                    }
                    // All output senders dropped; nothing more can arrive.
                    Ok(None) => break,
                    Err(_elapsed) => {
                        if seen_output {
                            break;
                        }
                        eprintln!("Command produced no output within {STARTUP_TIMEOUT:?}.");
                        std::process::exit(1);
                    }
                }
            }
            Ok(Protocol::End) = control_rx.recv() => {
                // The command exited; grab any output still in flight.
                while let Ok(Some(bytes)) =
                    tokio::time::timeout(Duration::from_millis(100), shadow.channels.output_rx.recv())
                        .await
                {
                    advance(&mut shadow, &pty_input_tx, &bytes).await;
                }
                break;
            }
        }
    }

    let output = SBuffer::from_terminal(&shadow.terminal);
    let _ = shadow.kill();
    print!("{}", output);
}

/// Feed one PTY payload into the wezterm terminal, answering cursor position
/// requests (`ESC[6n`) so TUI apps waiting on the reply don't stall.
async fn advance(
    shadow: &mut ShadowTerminal,
    pty_input_tx: &tokio::sync::mpsc::Sender<BytesFromSTDIN>,
    bytes: &BytesFromPTY,
) {
    // Payloads are fixed-size arrays; the PTY reader marks the end with NUL.
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let bytes = &bytes[..len];
    shadow.terminal.advance_bytes(bytes);

    if bytes.windows(4).any(|window| window == b"\x1b[6n") {
        let pos = shadow.terminal.cursor_pos();
        let response = format!("\x1b[{};{}R", pos.y + 1, pos.x + 1);
        let mut payload: BytesFromSTDIN = [0; 128];
        payload[..response.len()].copy_from_slice(response.as_bytes());
        let _ = pty_input_tx.send(payload).await;
    }
}
