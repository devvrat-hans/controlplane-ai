//! Authentication and authorization.
//!
//! Authentication is a static token registry configured through
//! `MCP_AUTH_TOKENS` (`token:role` or `token:role:app_id|app_id`). Roles reuse
//! [`controlplane_common::types::UserRole`] so the MCP server and the rest of
//! the platform agree on what an `admin` or a `reviewer` may do.
//!
//! Authorization is capability based:
//!
//! | Capability | Roles |
//! |---|---|
//! | `Read`    | admin, reviewer, viewer |
//! | `Resolve` | admin, reviewer |
//! | `Write`   | admin |
//!
//! When a principal is scoped to a set of app ids, any tool that targets an app
//! must target one of those apps.

use uuid::Uuid;

use controlplane_common::types::UserRole;

use crate::error::McpError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Read,
    Resolve,
    Write,
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Capability::Read => "read",
            Capability::Resolve => "resolve",
            Capability::Write => "write",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Principal {
    /// A stable, non-secret identifier for the caller (e.g. token label).
    pub subject: String,
    pub role: UserRole,
    /// Restricts which apps this principal may act on, if configured.
    pub app_scope: Option<Vec<Uuid>>,
    pub anonymous: bool,
}

impl Principal {
    pub fn anonymous() -> Self {
        Self {
            subject: "anonymous".into(),
            role: UserRole::Viewer,
            app_scope: None,
            anonymous: true,
        }
    }

    pub fn new(subject: impl Into<String>, role: UserRole, app_scope: Option<Vec<Uuid>>) -> Self {
        Self {
            subject: subject.into(),
            role,
            app_scope,
            anonymous: false,
        }
    }

    pub fn authorize(&self, capability: Capability) -> Result<(), McpError> {
        let allowed = match capability {
            Capability::Read => true,
            Capability::Resolve => self.role.can_resolve_escalations(),
            Capability::Write => self.role.can_edit_policies(),
        };
        if allowed {
            Ok(())
        } else {
            Err(McpError::forbidden(capability.as_str()))
        }
    }

    /// Enforce app-scoped isolation for app-targeting tools.
    pub fn authorize_app(&self, app_id: Uuid) -> Result<(), McpError> {
        match &self.app_scope {
            Some(scope) if !scope.contains(&app_id) => Err(McpError::forbidden("app_scope")),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TokenEntry {
    pub label: String,
    pub token: String,
    pub role: UserRole,
    pub app_scope: Option<Vec<Uuid>>,
}

#[derive(Debug, Clone, Default)]
pub struct AuthRegistry {
    entries: Vec<TokenEntry>,
    allow_anonymous: bool,
}

impl AuthRegistry {
    pub fn new(allow_anonymous: bool) -> Self {
        Self {
            entries: Vec::new(),
            allow_anonymous,
        }
    }

    /// Parse the `MCP_AUTH_TOKENS` value.
    ///
    /// Entries are comma separated; each entry is `token:role` or
    /// `token:role:app_id|app_id`. Tokens must not contain `:` or `,`.
    pub fn from_config(allow_anonymous: bool, tokens: &str) -> Result<Self, McpError> {
        let mut entries = Vec::new();
        for (idx, raw) in tokens
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .enumerate()
        {
            let mut parts = raw.splitn(3, ':');
            let token = parts.next().unwrap_or_default().trim();
            let role = parts.next().unwrap_or_default().trim();
            let scope = parts.next().map(str::trim).filter(|s| !s.is_empty());

            if token.is_empty() || role.is_empty() {
                return Err(McpError::invalid_request(format!(
                    "Malformed MCP_AUTH_TOKENS entry #{idx}"
                )));
            }
            let role = parse_role(role).ok_or_else(|| {
                McpError::invalid_request(format!("Unknown role in MCP_AUTH_TOKENS entry #{idx}"))
            })?;
            let app_scope = match scope {
                Some(s) => Some(
                    s.split('|')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|id| {
                            Uuid::parse_str(id).map_err(|_| {
                                McpError::invalid_request(format!(
                                    "Invalid app id in MCP_AUTH_TOKENS entry #{idx}"
                                ))
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                None => None,
            };

            entries.push(TokenEntry {
                label: format!("token-{}", idx + 1),
                token: token.to_string(),
                role,
                app_scope,
            });
        }
        Ok(Self {
            entries,
            allow_anonymous,
        })
    }

    /// Resolve a bearer token to a principal, constant-time compared.
    pub fn resolve(&self, token: Option<&str>) -> Result<Principal, McpError> {
        if let Some(token) = token {
            for (idx, entry) in self.entries.iter().enumerate() {
                if constant_time_eq(entry.token.as_bytes(), token.as_bytes()) {
                    return Ok(Principal::new(
                        format!("token-{}", idx + 1),
                        entry.role,
                        entry.app_scope.clone(),
                    ));
                }
            }
            return Err(McpError::unauthorized());
        }

        if self.allow_anonymous {
            Ok(Principal::anonymous())
        } else {
            Err(McpError::unauthorized())
        }
    }

    pub fn has_entries(&self) -> bool {
        !self.entries.is_empty()
    }
}

pub fn parse_role(role: &str) -> Option<UserRole> {
    match role.trim().to_lowercase().as_str() {
        "admin" => Some(UserRole::Admin),
        "reviewer" => Some(UserRole::Reviewer),
        "viewer" => Some(UserRole::Viewer),
        _ => None,
    }
}

/// Length-independent comparison to avoid leaking token length via timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(tokens: &str) -> AuthRegistry {
        AuthRegistry::from_config(false, tokens).unwrap()
    }

    #[test]
    fn parses_token_role_pairs() {
        let reg = registry("abc:admin,def:reviewer");
        assert_eq!(reg.entries.len(), 2);
        assert_eq!(reg.entries[0].role, UserRole::Admin);
        assert_eq!(reg.entries[1].role, UserRole::Reviewer);
    }

    #[test]
    fn rejects_unknown_role() {
        assert!(AuthRegistry::from_config(false, "abc:superuser").is_err());
    }

    #[test]
    fn resolves_valid_token_only() {
        let reg = registry("abc:admin");
        let principal = reg.resolve(Some("abc")).unwrap();
        assert_eq!(principal.role, UserRole::Admin);
        assert!(reg.resolve(Some("wrong")).is_err());
        assert!(reg.resolve(None).is_err());
    }

    #[test]
    fn anonymous_only_when_allowed() {
        let reg = AuthRegistry::new(true);
        assert!(reg.resolve(None).unwrap().anonymous);
        assert!(AuthRegistry::new(false).resolve(None).is_err());
    }

    #[test]
    fn capability_matrix() {
        let viewer = Principal::new("v", UserRole::Viewer, None);
        assert!(viewer.authorize(Capability::Read).is_ok());
        assert!(viewer.authorize(Capability::Resolve).is_err());
        assert!(viewer.authorize(Capability::Write).is_err());

        let reviewer = Principal::new("r", UserRole::Reviewer, None);
        assert!(reviewer.authorize(Capability::Resolve).is_ok());
        assert!(reviewer.authorize(Capability::Write).is_err());

        let admin = Principal::new("a", UserRole::Admin, None);
        assert!(admin.authorize(Capability::Write).is_ok());
    }

    #[test]
    fn app_scope_is_enforced() {
        let app = Uuid::parse_str("10000000-0000-0000-0000-000000000001").unwrap();
        let other = Uuid::parse_str("10000000-0000-0000-0000-000000000002").unwrap();
        let principal = Principal::new("s", UserRole::Admin, Some(vec![app]));
        assert!(principal.authorize_app(app).is_ok());
        assert!(principal.authorize_app(other).is_err());
    }

    #[test]
    fn parses_app_scope() {
        let reg = registry(
            "abc:admin:10000000-0000-0000-0000-000000000001|10000000-0000-0000-0000-000000000002",
        );
        assert_eq!(reg.entries[0].app_scope.as_ref().unwrap().len(), 2);
    }
}
