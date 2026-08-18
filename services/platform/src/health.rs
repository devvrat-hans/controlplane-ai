use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ReadyResponse {
    pub status: &'static str,
    pub checks: ReadyChecks,
}

#[derive(Debug, Serialize)]
pub struct ReadyChecks {
    pub postgres: CheckStatus,
    pub nats: CheckStatus,
}

#[derive(Debug, Serialize)]
pub struct CheckStatus {
    pub status: &'static str,
    pub message: Option<String>,
}

impl CheckStatus {
    pub fn ok() -> Self {
        Self { status: "ok", message: None }
    }

    pub fn degraded(msg: impl Into<String>) -> Self {
        Self { status: "degraded", message: Some(msg.into()) }
    }

    pub fn unavailable(msg: impl Into<String>) -> Self {
        Self { status: "unavailable", message: Some(msg.into()) }
    }
}

pub fn health() -> HealthResponse {
    HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    }
}

pub async fn ready(pool: Option<&PgPool>, nats_healthy: bool) -> ReadyResponse {
    let postgres_status = match pool {
        Some(p) => {
            if crate::db::check_health(p).await {
                CheckStatus::ok()
            } else {
                CheckStatus::unavailable("Cannot reach PostgreSQL")
            }
        }
        None => CheckStatus::degraded("Running in memory mode"),
    };

    let nats_status = if nats_healthy {
        CheckStatus::ok()
    } else {
        CheckStatus::degraded("NATS not connected (using in-process bus)")
    };

    let overall = if postgres_status.status == "ok" {
        "ready"
    } else {
        "degraded"
    };

    ReadyResponse {
        status: overall,
        checks: ReadyChecks {
            postgres: postgres_status,
            nats: nats_status,
        },
    }
}
