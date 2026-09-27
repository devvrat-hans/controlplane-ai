use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type CorrelationId = Uuid;
pub type AppId = Uuid;
pub type TeamId = Uuid;
pub type UserId = Uuid;

/// Flat price billed for every request routed through the LLM proxy, in USD.
/// The single source of truth for cost accounting and the cost dashboard;
/// spend = request count × this value, independent of model or token count.
pub const COST_PER_REQUEST_USD: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Performance,
    Cost,
    Responsibility,
}

impl Axis {
    pub fn as_str(&self) -> &'static str {
        match self {
            Axis::Performance => "performance",
            Axis::Cost => "cost",
            Axis::Responsibility => "responsibility",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "performance" => Some(Axis::Performance),
            "cost" => Some(Axis::Cost),
            "responsibility" => Some(Axis::Responsibility),
            _ => None,
        }
    }
}

impl fmt::Display for Axis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Path {
    Fast,
    Shadow,
}

impl Path {
    pub fn as_str(&self) -> &'static str {
        match self {
            Path::Fast => "fast",
            Path::Shadow => "shadow",
        }
    }

    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "fast" => Path::Fast,
            "shadow" => Path::Shadow,
            _ => Path::Fast,
        }
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass = 0,
    Escalate = 1,
    Edit = 2,
    Block = 3,
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Edit => "edit",
            Outcome::Block => "block",
            Outcome::Escalate => "escalate",
        }
    }

    pub fn is_actionable(&self) -> bool {
        !matches!(self, Outcome::Pass)
    }

    pub fn should_notify(&self) -> bool {
        matches!(self, Outcome::Block | Outcome::Escalate)
    }

    pub fn worst(a: Self, b: Self) -> Self {
        if a >= b { a } else { b }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pass" => Some(Outcome::Pass),
            "edit" => Some(Outcome::Edit),
            "block" => Some(Outcome::Block),
            "escalate" => Some(Outcome::Escalate),
            _ => None,
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationStatus {
    Open,
    InReview,
    Resolved,
}

impl EscalationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            EscalationStatus::Open => "open",
            EscalationStatus::InReview => "in_review",
            EscalationStatus::Resolved => "resolved",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "open" => Some(EscalationStatus::Open),
            "in_review" => Some(EscalationStatus::InReview),
            "resolved" => Some(EscalationStatus::Resolved),
            _ => None,
        }
    }
}

impl fmt::Display for EscalationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Confirm,
    Override,
    Dismiss,
}

impl Resolution {
    pub fn as_str(&self) -> &'static str {
        match self {
            Resolution::Confirm => "confirm",
            Resolution::Override => "override",
            Resolution::Dismiss => "dismiss",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "confirm" => Some(Resolution::Confirm),
            "override" => Some(Resolution::Override),
            "dismiss" => Some(Resolution::Dismiss),
            _ => None,
        }
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Admin,
    Reviewer,
    Viewer,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::Admin => "admin",
            UserRole::Reviewer => "reviewer",
            UserRole::Viewer => "viewer",
        }
    }

    pub fn can_resolve_escalations(&self) -> bool {
        matches!(self, UserRole::Admin | UserRole::Reviewer)
    }

    pub fn can_edit_policies(&self) -> bool {
        matches!(self, UserRole::Admin)
    }
}

impl fmt::Display for UserRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_ordering() {
        assert!(Outcome::Block > Outcome::Edit);
        assert!(Outcome::Edit > Outcome::Escalate);
        assert!(Outcome::Escalate > Outcome::Pass);
    }

    #[test]
    fn outcome_worst_picks_higher() {
        assert_eq!(Outcome::worst(Outcome::Pass, Outcome::Block), Outcome::Block);
        assert_eq!(Outcome::worst(Outcome::Edit, Outcome::Escalate), Outcome::Edit);
    }

    #[test]
    fn axis_roundtrip_serde() {
        let json = serde_json::to_string(&Axis::Performance).unwrap();
        assert_eq!(json, "\"performance\"");
        let parsed: Axis = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Axis::Performance);
    }

    #[test]
    fn outcome_roundtrip_serde() {
        for outcome in [Outcome::Pass, Outcome::Edit, Outcome::Block, Outcome::Escalate] {
            let json = serde_json::to_string(&outcome).unwrap();
            let parsed: Outcome = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, outcome);
        }
    }
}
