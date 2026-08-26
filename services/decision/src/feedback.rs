use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

/// Reviewer precedent retrieved from the `reviewer_overrides` learning store.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Precedent {
    pub id: Uuid,
    pub call_id: Uuid,
    pub axis: String,
    pub model_outcome: String,
    pub reviewer_action: String,
    pub reviewer_reason: Option<String>,
    /// Trigram similarity 0.0–1.0 vs the query text.
    pub score: f64,
}

/// Find past reviewer decisions on content similar to the given text.
///
/// Uses pg_trgm trigram similarity on the stored request/response excerpts —
/// deterministic, no LLM, no external embedding service. Fails open: any
/// error returns an empty list so decisions are never blocked by learning
/// infrastructure.
///
/// `min_score` is a normalized similarity threshold (pg_trgm's `similarity()`
/// ranges 0.0–1.0; ≥0.3 is loosely similar, ≥0.6 is strongly similar).
pub async fn find_similar_precedents(
    pool: &PgPool,
    app_id: Option<Uuid>,
    text: &str,
    k: usize,
    min_score: f64,
) -> Vec<Precedent> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let query = "SELECT id, call_id, axis, model_outcome, reviewer_action, reviewer_reason, \
                GREATEST( \
                    COALESCE(similarity(response_excerpt, $1), 0), \
                    COALESCE(similarity(request_excerpt, $1), 0) \
                ) AS score \
         FROM reviewer_overrides \
         WHERE ($2::uuid IS NULL OR app_id = $2) \
           AND (COALESCE(similarity(response_excerpt, $1), 0) >= $3 \
                OR COALESCE(similarity(request_excerpt, $1), 0) >= $3) \
         ORDER BY score DESC \
         LIMIT $4".to_string();

    match sqlx::query_as::<_, Precedent>(&query)
        .bind(text)
        .bind(app_id)
        .bind(min_score)
        .bind(k as i64)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            warn!(error = %e, "Precedent retrieval failed — continuing without learned context");
            Vec::new()
        }
    }
}

/// Build a human-readable annotation from precedents that contradict or
/// support the current decision. This is "suggest mode": we never silently
/// change an outcome — the note travels inside the verdict reason so it is
/// visible on the dashboard and in the audit trail.
///
/// Returns `(annotation, precedent_ids)`; annotation is empty when nothing relevant.
pub fn annotate_from_precedents(
    final_outcome: &str,
    precedents: &[Precedent],
) -> (String, Vec<Uuid>) {
    let mut notes: Vec<String> = Vec::new();
    let mut ids: Vec<Uuid> = Vec::new();

    for p in precedents {
        ids.push(p.id);
        let pct = (p.score * 100.0).round() as i64;
        let action_past = match p.reviewer_action.as_str() {
            "override" => "overridden",
            "dismiss" => "dismissed",
            "confirm" => "confirmed",
            other => other,
        };
        match (final_outcome, p.reviewer_action.as_str()) {
            // Model flagged, but a very similar case was corrected by a reviewer
            ("escalate", "override") | ("escalate", "dismiss") | ("block", "override") | ("block", "dismiss") | ("edit", "override") => {
                notes.push(format!(
                    "⚠ {}%-similar past case was {} by a reviewer{}",
                    pct,
                    action_past,
                    p.reviewer_reason
                        .as_deref()
                        .map(|r| format!(": \"{}\"", truncate(r, 120)))
                        .unwrap_or_default(),
                ));
            }
            // Model passed, but reviewers confirmed similar content was a real issue
            ("pass", "confirm") => {
                notes.push(format!(
                    "⚠ {}%-similar past case was confirmed by a reviewer as a genuine issue{}",
                    pct,
                    p.reviewer_reason
                        .as_deref()
                        .map(|r| format!(": \"{}\"", truncate(r, 120)))
                        .unwrap_or_default(),
                ));
            }
            _ => {}
        }
    }

    if notes.is_empty() {
        (String::new(), ids)
    } else {
        (format!(" [Learned] {}", notes.join("; ")), ids)
    }
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn precedent(action: &str, score: f64, reason: Option<&str>) -> Precedent {
        Precedent {
            id: Uuid::now_v7(),
            call_id: Uuid::now_v7(),
            axis: "responsibility".to_string(),
            model_outcome: "escalate".to_string(),
            reviewer_action: action.to_string(),
            reviewer_reason: reason.map(|s| s.to_string()),
            score,
        }
    }

    #[test]
    fn no_precedents_gives_no_annotation() {
        let (note, ids) = annotate_from_precedents("escalate", &[]);
        assert!(note.is_empty());
        assert!(ids.is_empty());
    }

    #[test]
    fn contradicting_override_annotates() {
        let ps = vec![precedent("override", 0.82, Some("Stats were reliable"))];
        let (note, ids) = annotate_from_precedents("escalate", &ps);
        assert!(note.contains("[Learned]"));
        assert!(note.contains("82%-similar"));
        assert!(note.contains("overridden"));
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn confirming_agreement_does_not_annotate_when_flagged() {
        // Model escalated and reviewer confirmed → agreement, no note needed
        let ps = vec![precedent("confirm", 0.9, None)];
        let (note, _) = annotate_from_precedents("escalate", &ps);
        assert!(note.is_empty());
    }

    #[test]
    fn pass_with_confirmed_history_warns() {
        let ps = vec![precedent("confirm", 0.75, Some("Real PII leak"))];
        let (note, _) = annotate_from_precedents("pass", &ps);
        assert!(note.contains("confirmed"));
        assert!(note.contains("Real PII leak"));
    }
}
