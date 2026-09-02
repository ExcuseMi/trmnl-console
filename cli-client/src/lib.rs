//! Shared library code for the `trmnl-console` CLI and `trmnl-console-backend` scheduler.
//!
//! Both binaries capture terminal output into an [`sbuffer::SBuffer`] via
//! [`virtual_terminal::VirtualTerminal`] and send it to a TRMNL private plugin webhook
//! (see [`payload`] and [`webhook`]) using the payload format documented at
//! <https://docs.trmnl.com/go/private-plugins/webhooks>.

pub mod demo;
pub mod payload;
#[cfg(feature = "preview")]
pub mod preview_server;
#[cfg(feature = "relay")]
pub mod relay;
pub mod sbuffer;
pub mod terminal_subprocess;
pub mod virtual_terminal;
pub mod webhook;

use clap::Parser;
use std::env;
use std::process::ExitCode;

/// Every binary in this crate that may construct a [`virtual_terminal::VirtualTerminal`]
/// must call this first thing in `main()`, before parsing its own CLI arguments.
///
/// [`virtual_terminal::VirtualTerminal`] drives PTY subprocesses by re-invoking
/// [`std::env::current_exe`] (i.e. the binary currently running) with an internal,
/// undocumented flag; this checks for that flag and, if present, takes over the process
/// to drive the PTY, returning the process's final exit code. Otherwise it returns `None`
/// and the caller should proceed with its normal argument parsing.
pub async fn try_run_internal_subprocess() -> Option<ExitCode> {
    let mut raw_args = env::args();
    raw_args.next(); // argv0
    if Some(terminal_subprocess::INTERNAL_SUBPROCESS_MODE_FLAG) == raw_args.next().as_deref() {
        Some(terminal_subprocess::drive_terminal(terminal_subprocess::SubprocArgs::parse()).await)
    } else {
        None
    }
}
