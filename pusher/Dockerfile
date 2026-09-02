# Builds and runs trmnl-console-pusher: a scheduler that periodically captures a command's
# output (e.g. a tmux/Zellij pane) and pushes it to a TRMNL webhook-style endpoint - TRMNL's
# own, or a self-hosted trmnl-console-backend (see ../backend/).
#
# Build from the repository root:
#   docker build -f pusher/Dockerfile -t trmnl-console-pusher .
#
# tmux and Zellij are installed in the runtime image so `command:` entries in your job config
# can capture from either without you having to build your own image on top of this one. Neither
# is started automatically - if you want the pusher capturing a *live* session, run that session
# yourself (in this container or reachable from it) and point a job's `command` at it. See
# pusher/README.md and pusher/examples/ for how.

FROM rust:1-bookworm AS builder
WORKDIR /build
COPY cli-client/ cli-client/
WORKDIR /build/cli-client
# --no-default-features: skips the CLI's "preview" feature (and its dependency on the
# trmnl-framework git submodule) since the pusher doesn't use it.
RUN cargo build --release --no-default-features --bin trmnl-console-pusher

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        tmux \
        curl \
    && rm -rf /var/lib/apt/lists/*

ARG ZELLIJ_VERSION=0.41.2
RUN curl -fsSL "https://github.com/zellij-org/zellij/releases/download/v${ZELLIJ_VERSION}/zellij-x86_64-unknown-linux-musl.tar.gz" \
    | tar -xz -C /usr/local/bin \
    && chmod +x /usr/local/bin/zellij \
    && apt-get purge -y curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/cli-client/target/release/trmnl-console-pusher /usr/local/bin/trmnl-console-pusher

ENV TRMNL_CONSOLE_PUSHER_CONFIG=/etc/trmnl-console/pusher.yml
ENTRYPOINT ["trmnl-console-pusher"]
