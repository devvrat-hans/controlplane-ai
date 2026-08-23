use std::fmt;

/// Supported AI providers that the proxy can forward to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Anthropic,
    Gemini,
    OpenCode,
    Ollama,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderKind::Anthropic => write!(f, "anthropic"),
            ProviderKind::Gemini => write!(f, "gemini"),
            ProviderKind::OpenCode => write!(f, "opencode"),
            ProviderKind::Ollama => write!(f, "ollama"),
        }
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "anthropic" => Ok(ProviderKind::Anthropic),
            "gemini" => Ok(ProviderKind::Gemini),
            "opencode" | "oxalpha" => Ok(ProviderKind::OpenCode),
            "ollama" => Ok(ProviderKind::Ollama),
            _ => Err(format!("unknown provider: {s}")),
        }
    }
}

/// Trait for abstracting over different AI API providers.
///
/// Each provider has its own request format, auth mechanism, response format,
/// and token usage structure. This trait normalizes those differences so the
/// proxy, token counter, and shadow worker can work with any provider.
pub trait UpstreamProvider: Send + Sync {
    /// Returns the provider kind.
    fn kind(&self) -> ProviderKind;

    /// Rewrite the incoming request path for the upstream provider.
    ///
    /// For Anthropic: passthrough (e.g. `/v1/messages` stays as-is).
    /// For Gemini: rewrite `/v1/messages` → Gemini's generateContent endpoint.
    fn rewrite_path(&self, incoming_path: &str, model: &str) -> String;

    /// Inject authentication into an outgoing request builder.
    ///
    /// For Anthropic: adds `x-api-key` header and `anthropic-version` header.
    /// For Gemini: adds `key` query parameter.
    fn apply_auth<'a>(
        &self,
        request: reqwest::RequestBuilder,
        api_key: &'a str,
    ) -> reqwest::RequestBuilder;

    /// Extract the assistant's text response from the response body.
    fn extract_response_text(&self, body: &[u8]) -> Option<String>;

    /// Extract the user's prompt text from the request body.
    fn extract_request_prompt(&self, body: &[u8]) -> Option<String>;

    /// Extract RAG/system context from the request body, if present.
    fn extract_context(&self, body: &[u8]) -> Option<String>;

    /// Extract (input_tokens, output_tokens) from the response body.
    fn extract_token_usage(&self, body: &[u8]) -> (Option<i32>, Option<i32>);

    /// Extract the model name from the request body.
    fn extract_model(&self, body: &[u8]) -> Option<String>;
}
