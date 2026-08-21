//! Model metadata lookups — today just the **context window** of a
//! `provider::model` id, which sizes the auto-compact threshold
//! (`auto_compact_percent` of the window; see [`crate::settings`]).
//!
//! Resolution order:
//! 1. GAISe's bundled `model-registry.toml` (offline, instant): the
//!    vendor-documented window for every model the registry knows, including
//!    wildcard tags such as `ollama::qwen3:*`.
//! 2. A live `list_models` call to the model's provider, for models the registry
//!    has never heard of. Anthropic, Gemini, and Ollama report their windows
//!    through their model APIs; OpenAI does not. Bounded by a timeout and
//!    tolerant of every failure — an unknown window just means the caller falls
//!    back to an absolute token threshold.
//!
//! `None` always means *unknown*, never "unlimited".

use std::time::Duration;

use gaise_core::contracts::GaiseListModelsRequest;
use gaise_core::registry::ModelRegistry;
use gaise_core::GaiseClient;

/// Upper bound on the live provider lookup so an unreachable endpoint can't stall
/// the end of a turn.
const LIVE_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Split a routable `provider::model` id. `None` when there is no provider prefix.
pub fn split_model(model: &str) -> Option<(&str, &str)> {
    model
        .split_once("::")
        .filter(|(provider, id)| !provider.is_empty() && !id.is_empty())
}

/// The vendor-documented context window of `model` from the bundled registry.
pub fn registry_context_window(model: &str) -> Option<u64> {
    let (provider, id) = split_model(model)?;
    ModelRegistry::bundled()
        .find(provider, id)
        .and_then(|entry| entry.limits().context_window)
}

/// The context window of `model` as its provider reports it (registry-enriched by
/// the GAISe router). One listing call, filtered to the model's provider; Ollama
/// details are requested so `context_length` comes back for local models.
pub async fn live_context_window(client: &dyn GaiseClient, model: &str) -> Option<u64> {
    let (provider, id) = split_model(model)?;
    let request = GaiseListModelsRequest {
        provider: Some(provider.to_string()),
        include_details: true,
        ..Default::default()
    };
    let response = tokio::time::timeout(LIVE_LOOKUP_TIMEOUT, client.list_models(&request))
        .await
        .ok()?
        .ok()?;
    response
        .models
        .iter()
        .find(|m| m.id == model || m.id == id)
        .and_then(|m| m.limits.context_window)
}

/// Registry first, then the provider. See the module docs for the rules.
pub async fn context_window(client: &dyn GaiseClient, model: &str) -> Option<u64> {
    match registry_context_window(model) {
        Some(window) => Some(window),
        None => live_context_window(client, model).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_known_models_and_wildcards() {
        assert_eq!(
            registry_context_window("openai::gpt-5.4-mini"),
            Some(400_000)
        );
        assert!(registry_context_window("anthropic::claude-opus-5").is_some());
        // Wildcard registry tags cover concrete local tags.
        assert!(registry_context_window("ollama::qwen3:8b").is_some());
    }

    #[test]
    fn registry_is_unknown_for_unrouted_or_unlisted_models() {
        assert_eq!(registry_context_window("gpt-5.4-mini"), None);
        assert_eq!(registry_context_window("::"), None);
        assert_eq!(registry_context_window("openai::not-a-real-model-xyz"), None);
        // Embedding entries record a per-input limit, not a context window.
        assert_eq!(
            registry_context_window("openai::text-embedding-3-small"),
            None
        );
    }
}
