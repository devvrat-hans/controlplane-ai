use crate::provider::{ProviderKind, UpstreamProvider};
use crate::provider_anthropic::AnthropicProvider;
use crate::provider_gemini::GeminiProvider;
use crate::provider_ollama::OllamaProvider;
use crate::provider_opencode::OpenCodeProvider;

/// Create the appropriate provider based on the configured provider kind.
pub fn create_provider(kind: ProviderKind) -> Box<dyn UpstreamProvider> {
    match kind {
        ProviderKind::Anthropic => Box::new(AnthropicProvider),
        ProviderKind::Gemini => Box::new(GeminiProvider),
        ProviderKind::OpenCode => Box::new(OpenCodeProvider),
        ProviderKind::Ollama => Box::new(OllamaProvider),
    }
}
