use crate::provider::{ProviderKind, UpstreamProvider};
use crate::provider_anthropic::AnthropicProvider;
use crate::provider_gemini::GeminiProvider;

/// Create the appropriate provider based on the configured provider kind.
pub fn create_provider(kind: ProviderKind) -> Box<dyn UpstreamProvider> {
    match kind {
        ProviderKind::Anthropic => Box::new(AnthropicProvider),
        ProviderKind::Gemini => Box::new(GeminiProvider),
    }
}
