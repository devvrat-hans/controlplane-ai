use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub database_url: String,
    pub nats_url: Option<String>,
    pub proxy_listen_addr: String,
    pub upstream_base_url: String,
    pub dashboard_api_port: u16,
    pub jwt_secret: String,
    pub event_bus: EventBusMode,
    pub log_level: String,
    pub seed_demo_users: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EventBusMode {
    Inproc,
    Nats,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, anyhow::Error> {
        dotenvy::dotenv().ok();

        Ok(Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://controlplane:secret@localhost:5432/controlplane".into()),
            nats_url: std::env::var("NATS_URL").ok(),
            proxy_listen_addr: std::env::var("PROXY_LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8900".into()),
            upstream_base_url: std::env::var("UPSTREAM_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".into()),
            dashboard_api_port: std::env::var("DASHBOARD_API_PORT")
                .unwrap_or_else(|_| "8080".into())
                .parse()?,
            jwt_secret: std::env::var("JWT_SECRET")
                .unwrap_or_else(|_| "dev-secret-change-me".into()),
            event_bus: match std::env::var("EVENT_BUS").unwrap_or_else(|_| "inproc".into()).as_str() {
                "nats" => EventBusMode::Nats,
                _ => EventBusMode::Inproc,
            },
            log_level: std::env::var("LOG_LEVEL")
                .unwrap_or_else(|_| "info".into()),
            seed_demo_users: std::env::var("SEED_DEMO_USERS")
                .unwrap_or_else(|_| "true".into())
                .parse()
                .unwrap_or(true),
        })
    }

    pub fn is_nats_mode(&self) -> bool {
        self.event_bus == EventBusMode::Nats
    }
}
