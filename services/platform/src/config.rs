use serde::Deserialize;

use controlplane_common::provider::ProviderKind;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub database_url: String,
    pub nats_url: Option<String>,
    pub proxy_listen_addr: String,
    pub upstream_base_url: String,
    pub upstream_api_key: String,
    pub upstream_provider: ProviderKind,
    pub upstream_model: String,
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

        let upstream_provider = std::env::var("UPSTREAM_PROVIDER")
            .unwrap_or_else(|_| "ollama".into())
            .parse::<ProviderKind>()
            .unwrap_or(ProviderKind::Ollama);

        // Auto-detect upstream base URL based on provider if not explicitly set
        let upstream_base_url = std::env::var("UPSTREAM_BASE_URL").ok().unwrap_or_else(|| {
            match upstream_provider {
                ProviderKind::Anthropic => "https://api.anthropic.com".to_string(),
                ProviderKind::Gemini => "https://generativelanguage.googleapis.com".to_string(),
                ProviderKind::OpenCode => "https://opencode.ai".to_string(),
                ProviderKind::Ollama => "http://localhost:11434".to_string(),
            }
        });

        let upstream_api_key = std::env::var("UPSTREAM_API_KEY")
            .unwrap_or_default();

        let upstream_model = std::env::var("UPSTREAM_MODEL")
            .unwrap_or_else(|_| match upstream_provider {
                ProviderKind::Anthropic => "claude-sonnet-4-20250514".to_string(),
                ProviderKind::Gemini => "gemini-2.0-flash".to_string(),
                ProviderKind::OpenCode => "mimo-v2.5-free".to_string(),
                ProviderKind::Ollama => "qwen2.5:1.5b".to_string(),
            });

        Ok(Self {
            database_url: std::env::var("DATABASE_URL").unwrap_or_default(),
            nats_url: std::env::var("NATS_URL").ok(),
            proxy_listen_addr: std::env::var("PROXY_LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8900".into()),
            upstream_base_url,
            upstream_api_key,
            upstream_provider,
            upstream_model,
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
