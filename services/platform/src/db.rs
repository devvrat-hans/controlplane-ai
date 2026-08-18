use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::info;

pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    info!(url = %redact_url(database_url), "Connecting to PostgreSQL");

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .idle_timeout(std::time::Duration::from_secs(300))
        .connect(database_url)
        .await?;

    info!("PostgreSQL connection pool created");
    Ok(pool)
}

pub async fn check_health(pool: &PgPool) -> bool {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .is_ok()
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    info!("Running database migrations...");
    sqlx::migrate!("../../infra/migrations")
        .run(pool)
        .await
        .map_err(|e| sqlx::Error::Configuration(e.into()))?;
    info!("Migrations complete");
    Ok(())
}

fn redact_url(url: &str) -> String {
    if let Some(at_pos) = url.find('@') {
        if let Some(proto_end) = url.find("://") {
            return format!("{}://***@{}", &url[..proto_end], &url[at_pos + 1..]);
        }
    }
    "***".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_url_hides_credentials() {
        let url = "postgres://user:password@localhost:5432/db";
        let redacted = redact_url(url);
        assert_eq!(redacted, "postgres://***@localhost:5432/db");
        assert!(!redacted.contains("password"));
    }
}
