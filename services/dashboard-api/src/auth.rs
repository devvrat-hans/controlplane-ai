use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

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
pub async fn auth_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "demo-secret-key-change-in-prod".to_string());

    if let Some(auth_header) = request.headers().get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                let validation = Validation::default();
                let key = DecodingKey::from_secret(jwt_secret.as_bytes());

                match decode::<Claims>(token, &key, &validation) {
                    Ok(_token_data) => {
                        // Token valid — proceed
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

    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "demo-secret-key-change-in-prod".to_string());
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
