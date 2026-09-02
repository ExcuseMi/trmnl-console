# Builds and runs trmnl-console-backend: a self-hosted stand-in for TRMNL's own
# private-plugin webhook endpoint (accepts webhook-shaped pushes, serves them back for
# polling). See backend/README.md.
#
# Build from the repository root:
#   docker build -f backend/Dockerfile -t trmnl-console-backend .

FROM rust:1-bookworm AS builder
WORKDIR /build
COPY cli-client/ cli-client/
WORKDIR /build/cli-client
# --no-default-features: skips the CLI's "preview" feature (and its dependency on the
# trmnl-framework git submodule); --features backend pulls in just what this needs.
RUN cargo build --release --no-default-features --features backend --bin trmnl-console-backend

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/cli-client/target/release/trmnl-console-backend /usr/local/bin/trmnl-console-backend

ENV BACKEND_BIND=0.0.0.0:8080
ENV BACKEND_STATE_DIR=/data
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["trmnl-console-backend"]
