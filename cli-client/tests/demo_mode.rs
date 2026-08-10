//! Input "demo mode": no COMMAND and stdin IS a TTY — the binary renders an
//! example pattern. Tested through a real PTY (portable-pty) so the actual
//! TTY-detection branch runs.
//!
//! Caveat: portable-pty attaches stdin AND stdout/stderr of the child to the
//! PTY slave (per-fd splitting isn't exposed by its portable API). Output
//! therefore arrives ONLCR-translated and with stderr merged in, so the
//! assertions here are deliberately loose. If exact demo HTML ever needs
//! pinning, a Unix-only openpty with a raw-fd stdin would be the follow-up.

mod common;

use std::io::Read;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

#[test]
fn demo_mode_renders_example() {
    // CONTRACT: the demo pattern and its snapshot timing are unspecified —
    // tighten these assertions once the demo renderer exists. --wait-time
    // bounds the run in case demo mode waits for a timeout before
    // snapshotting.
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty failed");

    let mut cmd = CommandBuilder::new(common::BIN);
    cmd.args(["-w", "20", "-h", "5", "--wait-time", "2"]);
    let mut child = pair.slave.spawn_command(cmd).expect("spawn in PTY failed");
    // Close our slave handle so the master sees EOF once the child exits.
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let output_thread = std::thread::spawn(move || {
        // Manual read loop: read_to_end's buffer contents are unspecified on
        // error, and PTY masters commonly report EIO (not EOF) when the last
        // slave closes.
        let mut all = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => all.extend_from_slice(&buf[..n]),
            }
        }
        String::from_utf8_lossy(&all).into_owned()
    });

    // portable-pty's Child has no wait-with-timeout; poll under a deadline.
    let deadline = Instant::now() + common::HARD_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().expect("try_wait failed") {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("demo mode did not exit within {:?}", common::HARD_TIMEOUT);
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    drop(pair.master);
    // The outer PTY translates '\n' to "\r\n"; normalize before asserting.
    let output = output_thread.join().unwrap().replace("\r\n", "\n");

    assert!(
        status.success(),
        "demo mode exited with {status:?}; output: {output:?}"
    );
    let trimmed = output.trim();
    assert!(
        !trimmed.is_empty(),
        "demo mode should print an HTML snapshot"
    );
    assert!(
        !trimmed.contains("error:"),
        "output looks like a CLI error: {trimmed:?}"
    );
    // Guard against the current arg-parsing stub, which debug-prints Args and
    // would otherwise sneak past the loose assertions above.
    assert!(
        !trimmed.contains("Args {"),
        "binary still prints the debug stub instead of a demo snapshot"
    );
}
