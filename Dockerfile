# Multi-stage build: api + orchestrator binaries, slim runtime.
# rust:1-slim tracks current stable; deps in the tree require edition2024
# (Cargo >= 1.85), so don't pin below that.
FROM rust:1-slim AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY actions ./actions
COPY harness ./harness
COPY fixtures ./fixtures
# Only the api binary ships in this image: scheduled ingest runs in GitHub
# Actions (.github/workflows/ingest.yml), so building the orchestrator here
# would only slow down free-tier deploys.
RUN cargo build --release -p api

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd -m groundwork
WORKDIR /app
COPY --from=builder /app/target/release/api /usr/local/bin/
COPY ui ./ui
COPY fixtures ./fixtures
COPY actions ./actions
USER groundwork
ENV API_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["api"]
