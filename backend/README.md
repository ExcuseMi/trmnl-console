# trmnl-console-backend

A small scheduler that periodically captures a command's output (a tmux/Zellij pane, or any
other one-shot command) and pushes it to your "Console / Terminal Output" TRMNL plugin, using
the same TRMNL webhook-style payload (`merge_variables`) as the `trmnl-console` CLI's `--url`
mode - see <https://docs.trmnl.com/go/private-plugins/webhooks>.

This exists so you don't have to run `trmnl-console` by hand (or wire up your own cron job
around it) every time you want a session pushed to your device, and so you don't have to know
the exact device/orientation/scale up front: a job captures its command's output at every size
listed in its config and sends them all as "variants" in one payload; the plugin recipe
(`plugin/src/shared.liquid`) measures the screen space it's actually given at render time and
picks whichever variant fits best.

## Quick start

```bash
cp backend.example.yml backend.yml
$EDITOR backend.yml   # set webhook_url and at least one job
docker compose -f docker-compose.yml up --build
```

Or run it once (without a schedule) to sanity-check a config before leaving it running:

```bash
trmnl-console-backend --config backend.yml --once
```

See `examples/tmux.yml`, `examples/zellij.yml` and `examples/generic-command.yml` for focused,
copy-pasteable configs.

## How a job works

Each tick:

1. `command` is run once through `sh -c`, and its stdout is captured.
2. Those same captured bytes are replayed into a fresh virtual terminal for every size listed
   in `sizes`, producing one "variant" (id + width + scale + rendered content) per size.
3. If the combined payload would be over `max_payload_bytes`, the largest variants are dropped
   (one at a time) until it fits, or only one is left.
4. One `POST` is sent to `webhook_url` with `bar` and all surviving `variants`.

Capturing once and replaying it into several virtual terminals (rather than re-running
`command` once per size) keeps every variant consistent with each other, and is cheap even for
many sizes since a dump command like `tmux capture-pane` does no real work beyond reading
already-buffered pane content.

This model fits a **one-shot dump** - tmux/Zellij pane content, `df`, `git log`, etc. It does
not fit a *live*, redrawing TUI (`htop`, `btop`, ...) that needs to be driven inside a real PTY
while it runs; for those, keep using the `trmnl-console` CLI directly (see the top-level
README), on its own cron/systemd timer.

## Config reference

```yaml
# Default webhook URL for jobs that don't set their own `webhook_url`. At least one of
# top-level webhook_url or every job's own webhook_url must be set.
webhook_url: "https://usetrmnl.com/api/custom_plugins/YOUR-UUID-HERE"

jobs:
  - name: my-job                  # used only for logging
    command: "tmux capture-pane -t main -p -e"   # run via `sh -c`, stdout is captured
    interval_seconds: 300         # seconds between runs
    webhook_url: "..."            # optional, overrides the top-level default for this job
    pass_stderr: false            # also feed stderr into the captured terminal
    wait_time_seconds: 2.0        # settle time per size after the captured bytes are fed in
    bar:                          # optional; omit entirely to hide the bottom bar
      left: "tmux: main"
      right: null
      icon: null
    sizes:                        # optional; defaults to the full device table below
      - id: trmnl-og-landscape-x1 # arbitrary label, only used for logging
        width: 111
        height: 29
        scale: 1                  # optional, defaults to 1
    max_payload_bytes: 2000       # optional, defaults to 2000 (TRMNL's free-tier limit; 5000 on TRMNL+)
```

`sizes` defaults to the full device table from the top-level README's "Devices" section (10
entries: TRMNL OG landscape x1-3, TRMNL X landscape x1-4, TRMNL X portrait x1-3) if omitted.
That maximizes how well the picked variant fits *some* device without you having to know which
one - but sending 10 variants is much more likely to exceed the payload size limit for anything
but sparse output. Narrowing `sizes` to the device(s) you actually own (as in the examples)
keeps the payload small and leaves headroom for real content.

## Rate and size limits

TRMNL allows up to 12 webhooks/hour per plugin (30/hour on TRMNL+) and rejects payloads over
2kb (5kb on TRMNL+) - see <https://docs.trmnl.com/go/private-plugins/webhooks>. Keep
`interval_seconds` at 300 or higher unless you're on TRMNL+, and watch the logs: a job that's
dropping size variants or still over budget after dropping down to one logs a warning each
tick.

Two jobs sharing the same plugin instance (same `webhook_url`) overwrite each other's content
on every payload - `merge_variables` replaces plugin data wholesale by default. Point each job
at its own plugin instance's webhook URL unless you specifically want them to share one screen.

## Reaching tmux/Zellij sessions from Docker

The backend needs to reach whichever tmux/Zellij *server* your sessions actually live on. If
that's on the same host you're running Docker on, mount that server's socket directory into the
container - see the commented-out volumes in `docker-compose.yml`. tmux's default socket lives
at `/tmp/tmux-<uid>/`; Zellij's at `$XDG_RUNTIME_DIR/zellij/<version>/` (commonly
`/run/user/<uid>/zellij/`). Run the container as the same UID that owns the socket.

If your sessions live elsewhere (another host, a different container), point `command:` at
whatever reaches them instead - e.g. `ssh other-host tmux capture-pane -t main -p -e` - and
mount SSH credentials into the container the same way.

**Zellij-specific caveat, verified against zellij 0.42:** `zellij action dump-screen` (and
every other `zellij action` subcommand) only works while a client is actively attached to the
target session - a session with no attached client has nothing to dump. This is a real
difference from tmux, whose `capture-pane` works fine against a fully detached session. See
`examples/zellij.yml` for a workaround (keeping a client attached in its own background tmux
pane) - if you don't specifically need Zellij, tmux is the more robust choice for unattended
capture.

## Running natively (without Docker)

```bash
cd cli-client
cargo build --release --no-default-features --bin trmnl-console-backend
./target/release/trmnl-console-backend --config /path/to/backend.yml
```

`--no-default-features` skips the CLI's `preview` feature (and its dependency on the
`trmnl-framework` git submodule), which the backend doesn't use.

## CLI flags

```
trmnl-console-backend [--config <PATH>] [--once]
```

- `--config`/`-c` (or the `TRMNL_CONSOLE_BACKEND_CONFIG` env var, which the Docker image sets
  to `/etc/trmnl-console/backend.yml` by default): path to the YAML config.
- `--once`: run every job a single time immediately, then exit, instead of starting the
  schedule. Useful to check a config actually sends what you expect before leaving it running.
