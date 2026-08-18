use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(true)
        .init();

    tracing::info!("ControlPlane.ai gateway starting...");

    let config = controlplane_platform::config::AppConfig::from_env()?;
    tracing::info!(
        proxy_addr = %config.proxy_listen_addr,
        api_port = config.dashboard_api_port,
        upstream = %config.upstream_base_url,
        event_bus = ?config.event_bus,
        "Configuration loaded"
    );

    // TODO: Initialize database pool
    // let pool = controlplane_platform::db::create_pool(&config.database_url).await?;

    // TODO: Initialize event bus (NATS or in-process)
    // TODO: Start proxy listener
    // TODO: Start dashboard API server
    // TODO: Start shadow-path worker
    // TODO: Start notification worker

    tracing::info!("All services started. Press Ctrl+C to shut down.");

    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down gracefully...");

    Ok(())
}
