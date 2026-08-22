FROM rust:1-slim AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY services/ services/
COPY infra/migrations/ infra/migrations/

RUN cargo build --release -p controlplane-gateway

# --- Runtime stage ---
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/controlplane-gateway /usr/local/bin/controlplane-gateway

EXPOSE 8900 8081

ENV RUST_LOG=controlplane=info,tower_http=info
ENV EVENT_BUS=inproc

ENTRYPOINT ["controlplane-gateway"]
