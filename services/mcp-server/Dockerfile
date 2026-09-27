# MCP server image — builds the `controlplane-mcp` binary from the workspace.
#
# Build context is the repository root (same as the gateway image) so the
# workspace manifests and the `controlplane-common` dependency resolve:
#
#   docker build -f services/mcp-server/Dockerfile -t controlplane-mcp .

FROM rust:1-slim AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY services/ services/
# `sqlx::migrate!("../../infra/migrations")` in services/platform is a compile-time
# macro; this crate does not depend on platform today, but copy it so a future
# dependency edge cannot break the build.
COPY infra/migrations/ infra/migrations/

RUN cargo build --release -p controlplane-mcp-server

# --- Runtime stage ---
FROM debian:bookworm-slim

# curl is only used for the container healthcheck below.
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/controlplane-mcp /usr/local/bin/controlplane-mcp

# 8090 is the Streamable HTTP transport. The stdio transport does not listen.
EXPOSE 8090

ENV MCP_TRANSPORT=http
ENV MCP_HTTP_ADDR=0.0.0.0:8090
ENV RUST_LOG=controlplane_mcp_server=info

HEALTHCHECK --interval=10s --timeout=5s --start-period=5s --retries=5 \
    CMD curl -fsS http://127.0.0.1:8090/health || exit 1

ENTRYPOINT ["controlplane-mcp"]
