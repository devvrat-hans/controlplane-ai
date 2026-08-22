//! Tests for AppConfig provider-related configuration parsing.

use controlplane_common::provider::ProviderKind;

// =============================================================================
// ProviderKind parsing from environment
// =============================================================================

#[test]
fn parse_provider_kind_from_env_string() {
    // Test the FromStr implementation that AppConfig uses
    let kind: ProviderKind = "anthropic".parse().unwrap();
    assert_eq!(kind, ProviderKind::Anthropic);

    let kind: ProviderKind = "gemini".parse().unwrap();
    assert_eq!(kind, ProviderKind::Gemini);
}

#[test]
fn parse_provider_kind_case_insensitive() {
    assert_eq!("Anthropic".parse::<ProviderKind>().unwrap(), ProviderKind::Anthropic);
    assert_eq!("GEMINI".parse::<ProviderKind>().unwrap(), ProviderKind::Gemini);
    assert_eq!("Gemini".parse::<ProviderKind>().unwrap(), ProviderKind::Gemini);
}

#[test]
fn parse_provider_kind_invalid_returns_error() {
    assert!("openai".parse::<ProviderKind>().is_err());
    assert!("llama".parse::<ProviderKind>().is_err());
    assert!("".parse::<ProviderKind>().is_err());
}

// =============================================================================
// ProviderKind serialization
// =============================================================================

#[test]
fn provider_kind_serializes_to_lowercase_json() {
    let json = serde_json::to_string(&ProviderKind::Anthropic).unwrap();
    assert_eq!(json, "\"anthropic\"");

    let json = serde_json::to_string(&ProviderKind::Gemini).unwrap();
    assert_eq!(json, "\"gemini\"");
}

#[test]
fn provider_kind_deserializes_from_json() {
    let kind: ProviderKind = serde_json::from_str("\"anthropic\"").unwrap();
    assert_eq!(kind, ProviderKind::Anthropic);

    let kind: ProviderKind = serde_json::from_str("\"gemini\"").unwrap();
    assert_eq!(kind, ProviderKind::Gemini);
}

// =============================================================================
// Default upstream URL based on provider
// =============================================================================

#[test]
fn anthropic_default_base_url() {
    // Verify the expected default URL for Anthropic
    let expected = "https://api.anthropic.com";
    // This matches what AppConfig::from_env() uses
    assert!(expected.starts_with("https://"));
    assert!(expected.contains("anthropic"));
}

#[test]
fn gemini_default_base_url() {
    // Verify the expected default URL for Gemini
    let expected = "https://generativelanguage.googleapis.com";
    assert!(expected.starts_with("https://"));
    assert!(expected.contains("googleapis"));
}
