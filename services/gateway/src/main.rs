use std::sync::Arc;

use anyhow::Result;
use tokio::sync::watch;
use tracing::info;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use controlplane_platform::config::{AppConfig, EventBusMode};
use controlplane_platform::db;
use controlplane_platform::messaging::{InProcessBus, NatsBus};

use controlplane_fast_path::{FastPathEngine, FastPathRuleSet, PolicyCache};
use controlplane_proxy::proxy_router;
use controlplane_proxy::handler::ProxyState;
use controlplane_common::create_provider;

use controlplane_dashboard_api::{dashboard_router, spawn_sse_bridge, DashboardState, InMemoryVerdictStore, SseBroadcaster};
use controlplane_shadow_analysis::{
    spawn_calibration_reloader, spawn_toggle_reloader, CalibrationStore, ShadowConfig,
    ShadowWorker, ToggleStore,
};
use controlplane_decision::{spawn_shadow_run_collector, spawn_verdict_collector, DecisionServiceState, VerdictAggregator, PolicyEngine};
use controlplane_cost_accounting::spawn_cost_tracker;
use controlplane_escalation::spawn_escalation_listener;
use controlplane_notification::spawn_notification_worker;
use controlplane_audit::spawn_audit_subscriber;
use controlplane_fast_path::spawn_policy_reloader;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,controlplane=debug")),
        )
        .with_target(true)
        .init();

    info!("ControlPlane.ai gateway starting...");

    // ─── Configuration ───────────────────────────────────────────────────
    let config = AppConfig::from_env()?;
    info!(
        proxy_addr = %config.proxy_listen_addr,
        api_port = config.dashboard_api_port,
        upstream = %config.upstream_base_url,
        provider = %config.upstream_provider,
        event_bus = ?config.event_bus,
        "Configuration loaded"
    );

    // ─── Database ────────────────────────────────────────────────────────
    let pool = if config.database_url.is_empty() {
        info!("No DATABASE_URL configured — running without database (demo-only mode)");
        None
    } else {
        match db::create_pool(&config.database_url).await {
            Ok(p) => {
                info!("PostgreSQL connected");
                if std::env::var("RUN_MIGRATIONS").unwrap_or_default() == "true" {
                    if let Err(e) = db::run_migrations(&p).await {
                        tracing::warn!(error = %e, "Migration run failed (may already be applied)");
                    }
                }
                Some(p)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to connect to PostgreSQL — continuing without database");
                None
            }
        }
    };

    // ─── Event Bus ───────────────────────────────────────────────────────
    let (publisher, subscriber): (
        Arc<dyn controlplane_platform::messaging::EventPublisher>,
        Arc<dyn controlplane_platform::messaging::EventSubscriber>,
    ) = match config.event_bus {
        EventBusMode::Nats => {
            let nats_url = config.nats_url.as_deref().unwrap_or("nats://localhost:4222");
            let bus = Arc::new(NatsBus::connect(nats_url).await?);
            info!("NATS event bus connected");
            (bus.clone(), bus)
        }
        EventBusMode::Inproc => {
            let bus = Arc::new(InProcessBus::new());
            info!("In-process event bus active (demo mode)");
            (bus.clone(), bus)
        }
    };

    // ─── Shutdown channel ────────────────────────────────────────────────
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // ─── Fast-Path Engine ────────────────────────────────────────────────
    let policy_cache = PolicyCache::new(FastPathRuleSet::default());
    let fast_path_engine = Arc::new(FastPathEngine::new(policy_cache));
    info!("Fast-path engine initialized");

    // ─── Provider ────────────────────────────────────────────────────────
    let provider = create_provider(config.upstream_provider);
    info!(provider = %config.upstream_provider, "Upstream provider initialized");

    // ─── Proxy Server ────────────────────────────────────────────────────
    // Default app_id for demo: ChatBot-Prod seeded in 009_seed_demo_data.sql
    let default_app_id: Uuid = "10000000-0000-0000-0000-000000000001".parse().unwrap();
    let proxy_state = Arc::new(ProxyState {
        upstream_base_url: config.upstream_base_url.clone(),
        upstream_api_key: config.upstream_api_key.clone(),
        default_model: config.upstream_model.clone(),
        provider,
        http_client: reqwest::Client::new(),
        fast_path: fast_path_engine.clone(),
        publisher: publisher.clone(),
        pool: pool.clone(),
        default_app_id,
    });

    let proxy_app = proxy_router(proxy_state);
    let proxy_addr: std::net::SocketAddr = config.proxy_listen_addr.parse()?;

    let proxy_listener = tokio::net::TcpListener::bind(proxy_addr).await
        .map_err(|e| anyhow::anyhow!("Failed to bind proxy on {}: {} (is another instance running?)", proxy_addr, e))?;
    info!(addr = %proxy_addr, "Proxy listening");

    let proxy_shutdown = shutdown_rx.clone();
    let proxy_handle = tokio::spawn(async move {
        axum::serve(proxy_listener, proxy_app)
            .with_graceful_shutdown(shutdown_signal(proxy_shutdown))
            .await
            .unwrap();
    });

    // ─── Dashboard API Server ────────────────────────────────────────────
    let broadcaster = SseBroadcaster::new(1024);
    let verdict_store = InMemoryVerdictStore::new();
    spawn_sse_bridge(
        broadcaster.clone(),
        subscriber.clone(),
        shutdown_rx.clone(),
        Some(verdict_store.clone()),
    );

    let dashboard_state = DashboardState {
        pool: pool.clone(),
        broadcaster: broadcaster.clone(),
        in_memory: verdict_store,
        publisher: Some(publisher.clone()),
    };
    let dashboard_app = dashboard_router(dashboard_state);
    let api_addr: std::net::SocketAddr = format!("0.0.0.0:{}", config.dashboard_api_port).parse()?;

    let api_listener = tokio::net::TcpListener::bind(api_addr).await
        .map_err(|e| anyhow::anyhow!("Failed to bind dashboard API on {}: {} (is another instance running?)", api_addr, e))?;
    info!(addr = %api_addr, "Dashboard API listening");

    let api_shutdown = shutdown_rx.clone();
    let api_handle = tokio::spawn(async move {
        axum::serve(api_listener, dashboard_app)
            .with_graceful_shutdown(shutdown_signal(api_shutdown))
            .await
            .unwrap();
    });

    // ─── Background Workers ──────────────────────────────────────────────

    // ─── Shadow analysis worker ──────────────────────────────────────
    // Shared toggle store: Policies page enable/disable switches hot-reload
    let toggle_store = ToggleStore::default();
    // Shared calibration store: fitted judge temperatures, inert until a fit exists.
    let calibration_store = CalibrationStore::new();
    let shadow_config = ShadowConfig {
        provider: config.upstream_provider,
        ..ShadowConfig::default()
    };
    let shadow_worker = ShadowWorker::new(
        subscriber.clone(),
        publisher.clone(),
        shadow_config,
    )
    .with_toggles(toggle_store.clone())
    .with_calibration(calibration_store.clone());
    let shadow_shutdown = shutdown_rx.clone();
    tokio::spawn(async move {
        shadow_worker.run(shadow_shutdown).await;
    });
    info!(
        laya_url = %std::env::var("LAYA_URL").unwrap_or_else(|_| "(unset)".to_string()),
        "Shadow analysis worker started"
    );

    if let Some(ref db_pool) = pool {
        // Policy hot-reload (polls DB for changes, swaps into fast-path cache)
        spawn_policy_reloader(
            db_pool.clone(),
            fast_path_engine.policy_cache.clone(),
            Default::default(),
            shutdown_rx.clone(),
        );
        info!("Policy reloader started");

        // Instant fast-path reload on policy update events (no 30s poll wait)
        let reload_metrics = Arc::new(controlplane_fast_path::ReloadMetrics::default());
        controlplane_fast_path::spawn_nats_reload_trigger(
            fast_path_engine.policy_cache.clone(),
            db_pool.clone(),
            subscriber.clone(),
            reload_metrics,
            None,
        );

        // Shadow check toggles: load from policies table, hot-reload on updates
        spawn_toggle_reloader(db_pool.clone(), toggle_store.clone(), subscriber.clone(), shutdown_rx.clone());

        // Judge calibration: load the fit at startup, then poll for a re-fit. Absent a
        // fit the store stays inert and judge probabilities stay raw (fail-safe).
        spawn_calibration_reloader(
            db_pool.clone(),
            calibration_store.clone(),
            std::time::Duration::from_secs(60),
            shutdown_rx.clone(),
        );

        // Decision service (aggregates verdicts from fast+shadow paths)
        let decision_state = Arc::new(DecisionServiceState {
            pool: db_pool.clone(),
            aggregator: VerdictAggregator::new(),
            policy_engine: PolicyEngine::new(db_pool.clone()),
            publisher: publisher.clone(),
        });
        spawn_verdict_collector(decision_state, subscriber.clone(), shutdown_rx.clone());
        spawn_shadow_run_collector(db_pool.clone(), subscriber.clone(), shutdown_rx.clone());
        info!("Decision verdict collector started");

        // Cost tracking worker
        spawn_cost_tracker(db_pool.clone(), subscriber.clone(), shutdown_rx.clone());
        info!("Cost tracking worker started");

        // Audit subscriber
        spawn_audit_subscriber(db_pool.clone(), subscriber.clone(), shutdown_rx.clone());
        info!("Audit subscriber started");

        // Escalation listener
        spawn_escalation_listener(db_pool.clone(), subscriber.clone(), publisher.clone(), shutdown_rx.clone());
        info!("Escalation listener started");
    } else {
        info!("DB-dependent workers skipped (no database)");
    }

    // Notification worker
    spawn_notification_worker(subscriber.clone(), shutdown_rx.clone());
    info!("Notification worker started");

    // ─── Ready ───────────────────────────────────────────────────────────
    info!("═══════════════════════════════════════════════════════════════");
    info!("  ControlPlane.ai is running");
    info!("  Proxy:         http://{}", config.proxy_listen_addr);
    info!("  Dashboard API: http://0.0.0.0:{}", config.dashboard_api_port);
    info!("  Event Bus:     {:?}", config.event_bus);
    info!("═══════════════════════════════════════════════════════════════");

    // ─── Wait for shutdown ───────────────────────────────────────────────
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received, stopping services...");

    // Signal all workers to stop
    let _ = shutdown_tx.send(true);

    // Wait for servers to finish
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        async {
            let _ = proxy_handle.await;
            let _ = api_handle.await;
        }
    ).await;

    info!("ControlPlane.ai shutdown complete.");
    Ok(())
}

async fn shutdown_signal(mut rx: watch::Receiver<bool>) {
    while !*rx.borrow() {
        if rx.changed().await.is_err() {
            break;
        }
    }
}


