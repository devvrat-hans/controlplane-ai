use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use controlplane_common::types::{Axis, Outcome};
use crate::types::{ShadowVerdict, DEFAULT_SHADOW_TIMEOUT_MS};

#[derive(Serialize)]
struct ScanRequest {
    text: String,
    prompt: Option<String>,
}

#[derive(Deserialize)]
struct PIIEntity {
    entity_type: String,
    #[allow(dead_code)]
    start: usize,
    #[allow(dead_code)]
    end: usize,
    score: f64,
    text: String,
}

#[derive(Deserialize)]
struct PIIResponse {
    entities: Vec<PIIEntity>,
    #[allow(dead_code)]
    anonymized_text: String,
    has_pii: bool,
    duration_ms: f64,
}

#[derive(Deserialize)]
struct ToxicityResponse {
    is_toxic: bool,
    score: f64,
    #[allow(dead_code)]
    sanitized_text: String,
    duration_ms: f64,
}

#[derive(Deserialize)]
struct BiasResponse {
    is_biased: bool,
    score: f64,
    #[allow(dead_code)]
    sanitized_text: String,
    duration_ms: f64,
}

/// `Ok(Some)` = finding, `Ok(None)` = scanned and clean, `Err` = the scan did not
/// happen (sidecar unreachable / error status / unreadable body). Callers fail open
/// on `Err`, but can now report it instead of mistaking an outage for a pass.
pub type ScanResult = Result<Option<ShadowVerdict>, String>;

pub struct GuardrailsClient {
    base_url: String,
    http: reqwest::Client,
}

impl GuardrailsClient {
    /// Client with the default timeout ([`DEFAULT_SHADOW_TIMEOUT_MS`]).
    pub fn new(base_url: &str) -> Self {
        Self::with_timeout_ms(base_url, DEFAULT_SHADOW_TIMEOUT_MS)
    }

    /// Client whose scans give up after `timeout_ms` (reported as an error, fail open).
    pub fn with_timeout_ms(base_url: &str, timeout_ms: u64) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(timeout_ms.max(1)))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn scan_pii(&self, text: &str) -> ScanResult {
        let url = format!("{}/scan/pii", self.base_url);
        let body = ScanRequest { text: text.to_string(), prompt: None };

        match self.http.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<PIIResponse>().await {
                    Ok(result) => {
                        debug!(
                            has_pii = result.has_pii,
                            entities = result.entities.len(),
                            duration_ms = result.duration_ms,
                            "Presidio PII scan complete"
                        );
                        if result.has_pii {
                            let entity_summary: Vec<String> = result.entities.iter()
                                .take(5)
                                .map(|e| format!("{}({})", e.entity_type, e.text))
                                .collect();

                            let confidence = result.entities.iter()
                                .map(|e| e.score as f32)
                                .fold(0.0f32, f32::max);

                            let outcome = if result.entities.len() >= 3 {
                                Outcome::Escalate
                            } else {
                                Outcome::Edit
                            };

                            Ok(Some(ShadowVerdict {
                                axis: Axis::Responsibility,
                                check_name: "presidio-pii".to_string(),
                                outcome,
                                confidence,
                                reason: format!(
                                    "Presidio detected {} PII entities: [{}]",
                                    result.entities.len(),
                                    entity_summary.join(", ")
                                ),
                                duration_ms: result.duration_ms,
                            }))
                        } else {
                            Ok(None)
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse PII response");
                        Err("guardrails sidecar returned an unreadable response".to_string())
                    }
                }
            }
            Ok(resp) => {
                warn!(status = %resp.status(), "Guardrails PII endpoint returned error");
                Err(format!("guardrails sidecar returned HTTP {}", resp.status().as_u16()))
            }
            Err(e) => {
                warn!(error = %e, "Failed to reach guardrails PII endpoint");
                Err("guardrails sidecar unreachable".to_string())
            }
        }
    }

    pub async fn scan_toxicity(&self, text: &str, prompt: Option<&str>) -> ScanResult {
        let url = format!("{}/scan/toxicity", self.base_url);
        let body = ScanRequest {
            text: text.to_string(),
            prompt: prompt.map(|s| s.to_string()),
        };

        match self.http.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ToxicityResponse>().await {
                    Ok(result) => {
                        debug!(
                            is_toxic = result.is_toxic,
                            score = result.score,
                            duration_ms = result.duration_ms,
                            "LLM Guard toxicity scan complete"
                        );
                        if result.is_toxic {
                            let outcome = if result.score > 0.9 {
                                Outcome::Block
                            } else if result.score > 0.7 {
                                Outcome::Escalate
                            } else {
                                Outcome::Edit
                            };

                            Ok(Some(ShadowVerdict {
                                axis: Axis::Responsibility,
                                check_name: "llm-guard-toxicity".to_string(),
                                outcome,
                                confidence: result.score as f32,
                                reason: format!(
                                    "LLM Guard detected toxic content (score: {:.2})",
                                    result.score
                                ),
                                duration_ms: result.duration_ms,
                            }))
                        } else {
                            Ok(None)
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse toxicity response");
                        Err("guardrails sidecar returned an unreadable response".to_string())
                    }
                }
            }
            Ok(resp) => {
                warn!(status = %resp.status(), "Guardrails toxicity endpoint returned error");
                Err(format!("guardrails sidecar returned HTTP {}", resp.status().as_u16()))
            }
            Err(e) => {
                warn!(error = %e, "Failed to reach guardrails toxicity endpoint");
                Err("guardrails sidecar unreachable".to_string())
            }
        }
    }

    pub async fn scan_bias(&self, text: &str, prompt: Option<&str>) -> ScanResult {
        let url = format!("{}/scan/bias", self.base_url);
        let body = ScanRequest {
            text: text.to_string(),
            prompt: prompt.map(|s| s.to_string()),
        };

        match self.http.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<BiasResponse>().await {
                    Ok(result) => {
                        debug!(
                            is_biased = result.is_biased,
                            score = result.score,
                            duration_ms = result.duration_ms,
                            "LLM Guard bias scan complete"
                        );
                        if result.is_biased {
                            let outcome = if result.score > 0.85 {
                                Outcome::Escalate
                            } else {
                                Outcome::Edit
                            };

                            Ok(Some(ShadowVerdict {
                                axis: Axis::Responsibility,
                                check_name: "llm-guard-bias".to_string(),
                                outcome,
                                confidence: result.score as f32,
                                reason: format!(
                                    "LLM Guard detected biased content (score: {:.2})",
                                    result.score
                                ),
                                duration_ms: result.duration_ms,
                            }))
                        } else {
                            Ok(None)
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse bias response");
                        Err("guardrails sidecar returned an unreadable response".to_string())
                    }
                }
            }
            Ok(resp) => {
                warn!(status = %resp.status(), "Guardrails bias endpoint returned error");
                Err(format!("guardrails sidecar returned HTTP {}", resp.status().as_u16()))
            }
            Err(e) => {
                warn!(error = %e, "Failed to reach guardrails bias endpoint");
                Err("guardrails sidecar unreachable".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unreachable_sidecar_is_an_error_not_a_pass() {
        // Port 1 on loopback refuses connections immediately.
        let client = GuardrailsClient::new("http://127.0.0.1:1");
        let result = client.scan_pii("hello").await;
        assert!(matches!(result.as_ref(), Err(e) if e == "guardrails sidecar unreachable"), "got {:?}", result.map(|v| v.is_some()));
    }
}
