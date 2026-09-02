# Builds and runs trmnl-console-backend: a scheduler that periodically captures a command's
# output (e.g. a tmux/Zellij pane) and pushes it to a TRMNL private plugin webhook.
#
# Build from the repository root:
#   docker build -f backend/Dockerfile -t trmnl-console-backend .
#
# tmux and Zellij are installed in the runtime image so `command:` entries in your job config
# can capture from either without you having to build your own image on top of this one. Neither
# is started automatically - if you want the backend capturing a *live* session, run that session
# yourself (in this container or reachable from it) and point a job's `command` at it. See
# backend/README.md and backend/examples/ for how.

FROM rust:1-bookworm AS builder
WORKDIR /build
COPY cli-client/ cli-client/
WORKDIR /build/cli-client
# --no-default-features: skips the CLI's "preview" feature (and its dependency on the
# trmnl-framework git submodule) since the backend doesn't use it.
RUN cargo build --release --no-default-features --bin trmnl-console-backend

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

COPY --from=builder /build/cli-client/target/release/trmnl-console-backend /usr/local/bin/trmnl-console-backend

ENV TRMNL_CONSOLE_BACKEND_CONFIG=/etc/trmnl-console/backend.yml
ENTRYPOINT ["trmnl-console-backend"]
