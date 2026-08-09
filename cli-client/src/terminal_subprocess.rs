use interprocess::local_socket::prelude::*;
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{GenericFilePath, tokio::Stream};
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitCode;
use tokio::io::AsyncWriteExt;

pub(crate) const INTERNAL_SUBPROCESS_MODE_FLAG: &str = "--internal-subprocess-mode";
pub(crate) const SOCKET_PATH: &str = "--socket-path";

#[derive(Debug, clap::Parser)]
pub struct SubprocArgs {
    #[arg(long)]
    #[allow(unused)] // this is never checked: we check if this flag exists in main.rs
    internal_subprocess_mode: bool,
    #[arg(long)]
    socket_path: PathBuf,
    command: Vec<String>,
}

impl SubprocArgs {
    pub fn cmd_mode(&self) -> bool {
        !self.command.is_empty()
    }
}

/// To be used by the subprocess that drives the shadow terminal.
///
/// Tasks:
/// - Depending on TerminalOpMode:
///   - cmd mode: Spawns cmd and forwards stderr to the parent
///   - read mode: Forwards all bytes read to `stdout`. See [`VirtualTerminal::run_with_async_reader`]
pub(crate) async fn drive_terminal(args: SubprocArgs) -> ExitCode {
    let cmd_mode = args.cmd_mode();

    let socket_name = args.socket_path.to_fs_name::<GenericFilePath>().unwrap();

    if cmd_mode {
        let exit_code = {
            let mut stream = Stream::connect(socket_name.clone()).await.unwrap();

            // spawn a command with tokio, write its stderr back to `stream`.
            let mut cmd = tokio::process::Command::new(&args.command[0]);
            cmd.args(&args.command[1..]);
            cmd.stdin(std::process::Stdio::inherit());
            cmd.stderr(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::inherit());
            let mut child = cmd.spawn().unwrap();
            let mut child_stderr = child.stderr.take().unwrap();
            let (_, exit_status) = tokio::join!(
                tokio::spawn(async move {
                    tokio::io::copy(&mut child_stderr, &mut stream)
                        .await
                        .unwrap();
                }),
                child.wait()
            );
            // if the process finished disconnect
            get_exit_code(exit_status.unwrap())
        };
        // reconnect and then send the exit code
        let mut stream = Stream::connect(socket_name).await.unwrap();
        stream.write_u8(exit_code).await.unwrap();
    } else {
        let mut stream = Stream::connect(socket_name.clone()).await.unwrap();

        let mut stdout = tokio::io::stdout();
        tokio::io::copy(&mut stream, &mut stdout).await.unwrap();
    }

    ExitCode::SUCCESS
}

fn get_exit_code(exit_status: std::process::ExitStatus) -> u8 {
    match exit_status.code() {
        None => {
            if cfg!(unix) {
                128 + exit_status
                    .signal()
                    .unwrap_or_default()
                    .try_into()
                    .unwrap_or(1)
            } else {
                1
            }
        }
        Some(ec) => ec.try_into().unwrap_or(1),
    }
}
