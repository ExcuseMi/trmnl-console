# Builds and runs trmnl-console-relay: an always-on HTTP service that accepts a TRMNL
# webhook-shaped payload (from trmnl-console / trmnl-console-backend) and serves it back to a
# TRMNL "polling" private plugin. See relay/README.md.
#
# Build from the repository root:
#   docker build -f relay/Dockerfile -t trmnl-console-relay .

FROM rust:1-bookworm AS builder
WORKDIR /build
COPY cli-client/ cli-client/
WORKDIR /build/cli-client
# --no-default-features: skips the CLI's "preview" feature (and its dependency on the
# trmnl-framework git submodule); --features relay pulls in just what the relay needs.
RUN cargo build --release --no-default-features --features relay --bin trmnl-console-relay

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/cli-client/target/release/trmnl-console-relay /usr/local/bin/trmnl-console-relay

ENV RELAY_BIND=0.0.0.0:8080
ENV RELAY_STATE_PATH=/data/state.json
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["trmnl-console-relay"]
