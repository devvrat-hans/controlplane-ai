use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

/// Extension key for injecting authenticated user claims into request extensions.
pub const USER_CLAIMS: axum::http::HeaderName = axum::http::HeaderName::from_static("x-user-claims");

/// Extract user_id (sub) from request extensions. Returns None if not authenticated.
pub fn get_user_id_from_request(request: &axum::extract::Request) -> Option<String> {
    request.extensions().get::<Claims>().map(|c| c.sub.clone())
}

/// Optional extractor for JWT Claims — returns None instead of failing
/// when no token is present (demo mode).
pub struct OptionalClaims(pub Option<Claims>);

impl<S> axum::extract::FromRequestParts<S> for OptionalClaims
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let claims = parts.extensions.get::<Claims>().cloned();
        Ok(OptionalClaims(claims))
    }
}

/// JWT claims for authenticated users.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,  // user_id
    pub email: String,
    pub role: String,
    pub exp: usize,
}

/// JWT auth middleware.
/// For demo purposes: if no Authorization header, allow through with anonymous role.
/// In production, this would reject unauthenticated requests.
///
/// NOTE: SSE/stream routes intentionally use the same middleware. EventSource
/// connections cannot send custom headers, so demo mode allows unauthenticated
/// streaming. When JWT_SECRET is set and a valid token is provided, the claims
/// are injected into request extensions for downstream handlers (e.g., reviewer
/// identity on escalation resolve).
pub async fn auth_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-me".to_string());

    if let Some(auth_header) = request.headers().get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                let validation = Validation::default();
                let key = DecodingKey::from_secret(jwt_secret.as_bytes());

                match decode::<Claims>(token, &key, &validation) {
                    Ok(token_data) => {
                        // Token valid — inject claims for downstream handlers
                        let mut request = request;
                        request.extensions_mut().insert(token_data.claims);
                        return Ok(next.run(request).await);
                    }
                    Err(_) => {
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                }
            }
        }
    }

    // Demo mode: allow unauthenticated access
    Ok(next.run(request).await)
}

/// Generate a demo JWT for testing.
pub fn generate_demo_token(user_id: &str, email: &str, role: &str) -> String {
    use jsonwebtoken::{encode, EncodingKey, Header};

    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        role: role.to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
    };

    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-me".to_string());
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_validate_token() {
        std::env::set_var("JWT_SECRET", "test-secret");
        let token = generate_demo_token("user-1", "test@example.com", "admin");
        assert!(!token.is_empty());

        let key = DecodingKey::from_secret(b"test-secret");
        let validation = Validation::default();
        let decoded = decode::<Claims>(&token, &key, &validation);
        assert!(decoded.is_ok());

        let claims = decoded.unwrap().claims;
        assert_eq!(claims.sub, "user-1");
        assert_eq!(claims.email, "test@example.com");
        assert_eq!(claims.role, "admin");
    }
}
