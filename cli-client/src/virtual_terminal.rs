//! Provides the virtual terminal.
//!
//! Internally both [`VirtualTerminal::run_with_cmd`] and [`VirtualTerminal::run_with_async_reader`]
//! spawn a terminal with a PTY attached to a subprocess of `trmnl-console` itself.
//!
//! This special subprocess forwards stderr back to the parent process, so it doensn't
//! end up inside the virtual terminal but instead on the stderr of the parent.
//! With `run_with_async_reader` the subprocess pushes the bytes of this async reader to
//! the virtual terminal's stdout.
//! The communication between the subprocess and the virtual terminal is done via a local socket
//! / named pipe.
//!
//! See terminal_subprocess.rs for more details.
// TODO: consider working directly with raw wezterm and implementing pty driving ourselves.

use crate::sbuffer::SBuffer;
use crate::terminal_subprocess::{INTERNAL_SUBPROCESS_MODE_FLAG, SOCKET_PATH};
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{GenericFilePath, ListenerOptions};
use shadow_terminal::Protocol;
use shadow_terminal::pty::{BytesFromPTY, BytesFromSTDIN};
use shadow_terminal::shadow_terminal::{Config, ShadowTerminal};
use std::env::current_exe;
use std::ffi::OsString;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, Notify};
use tokio::time::timeout;

#[derive(Debug)]
pub enum SnapshotError {
    TerminalCrashed,
    TerminalTimedOut,
}

enum TerminalOpMode {
    Cmd(Vec<OsString>),
    Read(Pin<Box<dyn tokio::io::AsyncRead + Send>>),
}

impl TerminalOpMode {
    pub(crate) fn into_possible_parts(
        self,
    ) -> (
        Option<Pin<Box<dyn tokio::io::AsyncRead + Send>>>,
        Option<Vec<OsString>>,
    ) {
        match self {
            TerminalOpMode::Cmd(x) => (None, Some(x)),
            TerminalOpMode::Read(x) => (Some(x), None),
        }
    }
}

pub struct VirtualTerminal {
    command_finished: Arc<(Notify, AtomicBool)>,
    snapshot_req_tx: tokio::sync::oneshot::Sender<()>,
    snapshot_rx: tokio::sync::oneshot::Receiver<Result<SBuffer, u8>>,
    #[allow(unused)] // for Drop
    socket_tempdir: TempDir,
}

impl VirtualTerminal {
    /// Run the given command in a virtal terminal. The terminal and subprocess are started
    /// immediately.
    #[inline]
    pub async fn run_with_cmd(
        width: u16,
        height: u16,
        command: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> std::io::Result<Self> {
        Self::run_with_cmd_internal(
            width,
            height,
            TerminalOpMode::Cmd(command.into_iter().map(|x| x.as_ref().into()).collect()),
        )
        .await
    }

    /// Run a virtual terminal and send bytes from the given Read to the terminal.
    pub async fn run_with_async_reader(
        width: u16,
        height: u16,
        read: Pin<Box<dyn tokio::io::AsyncRead + Send>>,
    ) -> std::io::Result<Self> {
        Self::run_with_cmd_internal(width, height, TerminalOpMode::Read(read)).await
    }
    async fn run_with_cmd_internal(
        width: u16,
        height: u16,
        op: TerminalOpMode,
    ) -> std::io::Result<Self> {
        let exit_code: Arc<Mutex<Option<u8>>> = Default::default();

        let (read, cmd) = op.into_possible_parts();

        // Set up socket for child process
        let socket_tempdir = TempDir::new()?;
        let temp_socket_file_path = socket_tempdir.path().join("stdin.socket");
        let server = ListenerOptions::new()
            .name(
                temp_socket_file_path
                    .clone()
                    .to_fs_name::<GenericFilePath>()?,
            )
            .create_tokio()?;

        let exit_code2 = exit_code.clone();
        tokio::spawn(async move {
            let mut conn = match server.accept().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("failed to talk to subprocess: {e}");
                    return;
                }
            };
            // we only expect one connection, so we don't need to spawn a new task to loop
            if let Some(mut read) = read {
                tokio::io::copy(&mut read, &mut conn).await.unwrap();
            } else {
                // we are in cmd mode, let's listen for stderr
                tokio::io::copy(&mut conn, &mut tokio::io::stderr())
                    .await
                    .unwrap();

                // when we arrive here conn has closed, wait for a new connection to get the
                // exit code
                let mut conn = match server.accept().await {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("failed to talk to subprocess: {e}");
                        return;
                    }
                };
                *exit_code2.lock().await = Some(conn.read_u8().await.unwrap());
            }
        });

        let (snapshot_req_tx, mut snapshot_req_rx) = tokio::sync::oneshot::channel();
        let (snapshot_tx, snapshot_rx) = tokio::sync::oneshot::channel();

        let command_finished: Arc<(Notify, AtomicBool)> = Default::default();
        let command_finished2 = command_finished.clone();

        let slf = Self {
            command_finished,
            snapshot_req_tx,
            snapshot_rx,
            socket_tempdir,
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let mut command: Vec<OsString> = vec![
            current_exe()?.into(),
            INTERNAL_SUBPROCESS_MODE_FLAG.into(),
            SOCKET_PATH.into(),
            temp_socket_file_path.into(),
        ];
        if let Some(cmd) = cmd {
            command.push("--".into());
            command.extend(cmd);
        }

        std::thread::spawn(move || {
            let local = tokio::task::LocalSet::new();

            local.spawn_local(async move {
                // we use wezterm's output directly, these are unused.
                let (surface_tx, _surface_rx) = tokio::sync::mpsc::channel(1);
                let mut term = ShadowTerminal::new(
                    Config {
                        width,
                        height,
                        command,
                        scrollback_size: 0,
                        scrollback_step: 0,
                    },
                    surface_tx,
                );

                let (pty_input_tx, pty_input_rx) = tokio::sync::mpsc::channel(2048);
                let term_join = term.start(pty_input_rx);
                let mut control_rx = term.channels.control_tx.subscribe();
                let mut snapshot_tx = Some(snapshot_tx);

                loop {
                    tokio::select! {
                        received = term.channels.output_rx.recv() => {
                            match received {
                                Some(bytes) => {
                                    handle_byte_stream(&mut term, &pty_input_tx, &bytes).await;
                                }
                                None => {
                                    // Since we exited, we also need to send the snapshot already
                                    if take_snapshot(*exit_code.lock().await, &term, &mut snapshot_tx) {
                                        break;
                                    }
                                },
                            }
                        }
                        Ok(Protocol::End) = control_rx.recv() => {
                            while let Ok(Some(bytes)) =
                                timeout(Duration::from_millis(100), term.channels.output_rx.recv())
                                    .await
                            {
                                handle_byte_stream(&mut term, &pty_input_tx, &bytes).await;
                            }
                            // Since we exited, we also need to send the snapshot already
                            if take_snapshot(*exit_code.lock().await, &term, &mut snapshot_tx) {
                                break;
                            }
                            break;
                        }
                        _ = &mut snapshot_req_rx => {
                            if take_snapshot(*exit_code.lock().await, &term, &mut snapshot_tx) {
                                break;
                            }
                        }
                    }
                }
                let (notify, finished) = &*command_finished2;
                finished.store(true, Ordering::SeqCst);
                notify.notify_one();
                match term_join.await {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => eprintln!("pty error: {}", e),
                    Err(e) => eprintln!("virtual terminal join error: {}", e),
                }
            });

            rt.block_on(local);
        });

        Ok(slf)
    }

    /// Wait for the terminal to finish.
    pub async fn wait_for_finish(&self) {
        loop {
            let (notify, finished) = &*self.command_finished;
            if finished.load(Ordering::SeqCst) {
                return;
            }
            notify.notified().await;
        }
    }

    /// Whether the command has finished running. If `true` the terminal is also no longer running.
    #[allow(unused)]
    pub fn finished(&self) -> bool {
        let (_, finished) = &*self.command_finished;
        finished.load(Ordering::SeqCst)
    }

    /// Takes a snapshot of the terminal or receives the snapshot taken when the terminal was
    /// finished. This kills the terminal.
    pub async fn snapshot(self) -> Result<Result<SBuffer, u8>, SnapshotError> {
        // if this fails, the terminal may have already finished earlier, that's ok, it should
        // have sent us the snapshot already.
        self.snapshot_req_tx.send(()).ok();
        match timeout(Duration::from_millis(100), self.snapshot_rx).await {
            Ok(Ok(snapshot)) => Ok(snapshot),
            Ok(Err(_)) => Err(SnapshotError::TerminalCrashed),
            Err(_) => Err(SnapshotError::TerminalTimedOut),
        }
    }
}

fn take_snapshot(
    exit_code: Option<u8>,
    term: &ShadowTerminal,
    snapshot_tx: &mut Option<tokio::sync::oneshot::Sender<Result<SBuffer, u8>>>,
) -> bool {
    if let Some(snapshot_tx) = snapshot_tx.take() {
        let snapshot = match exit_code {
            Some(ec) if ec != 0 => Err(ec),
            _ => Ok(SBuffer::from_terminal(&term.terminal)),
        };

        snapshot_tx.send(snapshot).ok();
        true
    } else {
        false
    }
}

/// Feed one PTY payload into the wezterm terminal, answering cursor position
/// requests (`ESC[6n`) so TUI apps waiting on the reply don't stall.
async fn handle_byte_stream(
    term: &mut ShadowTerminal,
    pty_input_tx: &tokio::sync::mpsc::Sender<BytesFromSTDIN>,
    bytes: &BytesFromPTY,
) {
    // Payloads are fixed-size arrays; the PTY reader marks the end with NUL.
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let bytes = &bytes[..len];
    term.terminal.advance_bytes(bytes);

    if bytes.windows(4).any(|window| window == b"\x1b[6n") {
        let pos = term.terminal.cursor_pos();
        let response = format!("\x1b[{};{}R", pos.y + 1, pos.x + 1);
        let mut payload: BytesFromSTDIN = [0; 128];
        payload[..response.len()].copy_from_slice(response.as_bytes());
        let _ = pty_input_tx.send(payload).await;
    }
}
