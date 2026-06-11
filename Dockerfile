# Multi-stage build: api + orchestrator binaries, slim runtime.
FROM rust:1.83-slim AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY actions ./actions
COPY harness ./harness
COPY fixtures ./fixtures
RUN cargo build --release -p api -p orchestrator

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd -m groundwork
WORKDIR /app
COPY --from=builder /app/target/release/api /app/target/release/orchestrator /usr/local/bin/
COPY ui ./ui
COPY fixtures ./fixtures
COPY actions ./actions
USER groundwork
ENV API_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["api"]
