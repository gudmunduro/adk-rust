//! Model resolution — maps a [`ModelRef`] to a live `Arc<dyn Llm>`.
//!
//! The [`ModelResolver`] trait abstracts over model construction so that
//! the runtime can resolve any provider declaration into a callable model.
//! [`DefaultModelResolver`] implements the standard resolution logic:
//!
//! - **Shorthand** names are mapped to providers by prefix (`gemini-*` → Gemini,
//!   `gpt-*` → OpenAI, `claude-*` → Anthropic, etc.)
//! - **Structured** refs use the explicit `provider` field directly
//! - **OpenAI-compatible** refs construct a client with the given `base_url` + `api_key`

use std::sync::Arc;

use async_trait::async_trait;

use adk_core::Llm;

use crate::types::{ModelConfig, ModelRef, Provider};

/// Errors that can occur during model resolution.
#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    /// The model name prefix could not be mapped to a known provider.
    #[error(
        "cannot infer provider from model name \"{name}\". Expected prefix: gemini, gpt, claude, llama, mistral, or deepseek"
    )]
    UnknownProvider { name: String },

    /// The provider is recognized but model construction failed.
    #[error("failed to construct model for provider {provider:?}: {reason}")]
    ConstructionFailed { provider: Provider, reason: String },
}

/// Result alias for resolver operations.
pub type ResolverResult<T> = std::result::Result<T, ResolverError>;

/// Resolves a [`ModelRef`] into a live `Arc<dyn Llm>`.
///
/// Implementations may construct real provider clients or return pre-built
/// instances. The trait is async because construction may involve network
/// calls (e.g., verifying API keys or fetching model metadata).
///
/// # Example
///
/// ```rust,ignore
/// use adk_managed::resolver::{ModelResolver, DefaultModelResolver};
/// use adk_managed::types::ModelRef;
///
/// let resolver = DefaultModelResolver::new();
/// let model_ref = ModelRef::Shorthand("gemini-2.5-flash".to_string());
/// let llm = resolver.resolve(&model_ref).await?;
/// ```
#[async_trait]
pub trait ModelResolver: Send + Sync {
    /// Resolve a model reference into a callable LLM instance.
    async fn resolve(&self, model_ref: &ModelRef) -> ResolverResult<Arc<dyn Llm>>;
}

/// Infers the [`Provider`] from a shorthand model name by prefix matching.
///
/// # Mapping
///
/// | Prefix | Provider |
/// |--------|----------|
/// | `gemini` | Gemini |
/// | `gpt` | OpenAI |
/// | `claude` | Anthropic |
/// | `llama` | Ollama |
/// | `mistral` | Ollama |
/// | `deepseek` | Ollama |
///
/// Returns `Err(ResolverError::UnknownProvider)` if no prefix matches.
pub fn infer_provider(name: &str) -> ResolverResult<Provider> {
    let lower = name.to_lowercase();
    if lower.starts_with("gemini") {
        Ok(Provider::Gemini)
    } else if lower.starts_with("gpt") {
        Ok(Provider::Openai)
    } else if lower.starts_with("claude") {
        Ok(Provider::Anthropic)
    } else if lower.starts_with("llama")
        || lower.starts_with("mistral")
        || lower.starts_with("deepseek")
    {
        Ok(Provider::Ollama)
    } else {
        Err(ResolverError::UnknownProvider { name: name.to_string() })
    }
}

/// Default model resolver that uses prefix-based provider inference for
/// shorthand names and explicit provider fields for structured refs.
///
/// # Construction Behavior
///
/// The `DefaultModelResolver` currently returns a [`ResolverError::ConstructionFailed`]
/// for all resolved providers because actual model construction requires API keys
/// and network access. The important logic here is the *resolution* — mapping a
/// `ModelRef` to the correct provider. The platform layer is responsible for
/// injecting a custom `ModelResolver` that can actually construct models with
/// credentials.
///
/// # Example
///
/// ```rust,ignore
/// use adk_managed::resolver::DefaultModelResolver;
///
/// let resolver = DefaultModelResolver::new();
/// // In production, use a resolver that has access to credentials.
/// ```
#[derive(Debug, Clone, Default)]
pub struct DefaultModelResolver;

impl DefaultModelResolver {
    /// Create a new `DefaultModelResolver`.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ModelResolver for DefaultModelResolver {
    async fn resolve(&self, model_ref: &ModelRef) -> ResolverResult<Arc<dyn Llm>> {
        match model_ref {
            ModelRef::Shorthand(name) => {
                let provider = infer_provider(name)?;
                // In a real implementation, this would construct the appropriate
                // provider client using API keys from the environment or a
                // credential provider. For now, we return an error indicating
                // that actual construction is not yet wired up.
                Err(ResolverError::ConstructionFailed {
                    provider,
                    reason: format!(
                        "DefaultModelResolver cannot construct real models. \
                         Use a platform-provided resolver with credentials. \
                         Resolved provider: {provider:?}, model: {name}"
                    ),
                })
            }
            ModelRef::Structured { provider, model, .. } => {
                let model_name = match model {
                    ModelConfig::Name(name) => name.clone(),
                    ModelConfig::Compatible { model, base_url, .. } => {
                        // For OpenAI-compatible, we have all we need to construct
                        // a client (model + base_url + api_key), but actual
                        // construction is deferred to a credentialed resolver.
                        return Err(ResolverError::ConstructionFailed {
                            provider: *provider,
                            reason: format!(
                                "DefaultModelResolver cannot construct OpenAI-compatible \
                                 client. Model: {model}, base_url: {base_url}. \
                                 Use a platform-provided resolver with credentials."
                            ),
                        });
                    }
                };

                Err(ResolverError::ConstructionFailed {
                    provider: *provider,
                    reason: format!(
                        "DefaultModelResolver cannot construct real models. \
                         Use a platform-provided resolver with credentials. \
                         Provider: {provider:?}, model: {model_name}"
                    ),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- infer_provider tests ---

    #[test]
    fn test_infer_gemini_from_shorthand() {
        assert_eq!(infer_provider("gemini-2.5-flash").unwrap(), Provider::Gemini);
        assert_eq!(infer_provider("gemini-2.5-pro").unwrap(), Provider::Gemini);
        assert_eq!(infer_provider("gemini-3.1-flash-lite-preview").unwrap(), Provider::Gemini);
    }

    #[test]
    fn test_infer_openai_from_shorthand() {
        assert_eq!(infer_provider("gpt-4.1").unwrap(), Provider::Openai);
        assert_eq!(infer_provider("gpt-4o").unwrap(), Provider::Openai);
        assert_eq!(infer_provider("gpt-4.1-mini").unwrap(), Provider::Openai);
    }

    #[test]
    fn test_infer_anthropic_from_shorthand() {
        assert_eq!(infer_provider("claude-3.5-sonnet").unwrap(), Provider::Anthropic);
        assert_eq!(infer_provider("claude-4-opus").unwrap(), Provider::Anthropic);
    }

    #[test]
    fn test_infer_ollama_from_llama() {
        assert_eq!(infer_provider("llama-3.2-70b").unwrap(), Provider::Ollama);
    }

    #[test]
    fn test_infer_ollama_from_mistral() {
        assert_eq!(infer_provider("mistral-7b").unwrap(), Provider::Ollama);
        assert_eq!(infer_provider("mistral-large").unwrap(), Provider::Ollama);
    }

    #[test]
    fn test_infer_ollama_from_deepseek() {
        assert_eq!(infer_provider("deepseek-chat").unwrap(), Provider::Ollama);
        assert_eq!(infer_provider("deepseek-coder").unwrap(), Provider::Ollama);
    }

    #[test]
    fn test_infer_unknown_returns_error() {
        let result = infer_provider("some-random-model");
        assert!(result.is_err());
        match result.unwrap_err() {
            ResolverError::UnknownProvider { name } => {
                assert_eq!(name, "some-random-model");
            }
            _ => panic!("expected UnknownProvider error"),
        }
    }

    #[test]
    fn test_infer_case_insensitive() {
        assert_eq!(infer_provider("Gemini-2.5-flash").unwrap(), Provider::Gemini);
        assert_eq!(infer_provider("GPT-4.1").unwrap(), Provider::Openai);
        assert_eq!(infer_provider("Claude-3.5-sonnet").unwrap(), Provider::Anthropic);
        assert_eq!(infer_provider("LLAMA-3.2").unwrap(), Provider::Ollama);
        assert_eq!(infer_provider("DeepSeek-V3").unwrap(), Provider::Ollama);
    }

    // --- DefaultModelResolver tests ---

    #[tokio::test]
    async fn test_resolver_shorthand_gemini_infers_provider() {
        let resolver = DefaultModelResolver::new();
        let model_ref = ModelRef::Shorthand("gemini-2.5-flash".to_string());
        let result = resolver.resolve(&model_ref).await;

        // We expect ConstructionFailed (not UnknownProvider) because the
        // provider was successfully inferred but construction is stubbed.
        let err = result.err().expect("expected an error");
        match err {
            ResolverError::ConstructionFailed { provider, reason } => {
                assert_eq!(provider, Provider::Gemini);
                assert!(reason.contains("gemini-2.5-flash"));
            }
            e => panic!("expected ConstructionFailed, got: {e}"),
        }
    }

    #[tokio::test]
    async fn test_resolver_shorthand_openai_infers_provider() {
        let resolver = DefaultModelResolver::new();
        let model_ref = ModelRef::Shorthand("gpt-4.1".to_string());
        let result = resolver.resolve(&model_ref).await;

        let err = result.err().expect("expected an error");
        match err {
            ResolverError::ConstructionFailed { provider, .. } => {
                assert_eq!(provider, Provider::Openai);
            }
            e => panic!("expected ConstructionFailed, got: {e}"),
        }
    }

    #[tokio::test]
    async fn test_resolver_shorthand_anthropic_infers_provider() {
        let resolver = DefaultModelResolver::new();
        let model_ref = ModelRef::Shorthand("claude-3.5-sonnet".to_string());
        let result = resolver.resolve(&model_ref).await;

        let err = result.err().expect("expected an error");
        match err {
            ResolverError::ConstructionFailed { provider, .. } => {
                assert_eq!(provider, Provider::Anthropic);
            }
            e => panic!("expected ConstructionFailed, got: {e}"),
        }
    }

    #[tokio::test]
    async fn test_resolver_shorthand_unknown_returns_unknown_provider() {
        let resolver = DefaultModelResolver::new();
        let model_ref = ModelRef::Shorthand("totally-unknown-model".to_string());
        let result = resolver.resolve(&model_ref).await;

        let err = result.err().expect("expected an error");
        match err {
            ResolverError::UnknownProvider { name } => {
                assert_eq!(name, "totally-unknown-model");
            }
            e => panic!("expected UnknownProvider, got: {e}"),
        }
    }

    #[tokio::test]
    async fn test_resolver_structured_uses_provider_field() {
        let resolver = DefaultModelResolver::new();
        let model_ref = ModelRef::Structured {
            provider: Provider::Anthropic,
            model: ModelConfig::Name("claude-3.5-sonnet".to_string()),
            speed: None,
        };
        let result = resolver.resolve(&model_ref).await;

        let err = result.err().expect("expected an error");
        match err {
            ResolverError::ConstructionFailed { provider, reason } => {
                assert_eq!(provider, Provider::Anthropic);
                assert!(reason.contains("claude-3.5-sonnet"));
            }
            e => panic!("expected ConstructionFailed, got: {e}"),
        }
    }

    #[tokio::test]
    async fn test_resolver_structured_openai_compatible() {
        let resolver = DefaultModelResolver::new();
        let model_ref = ModelRef::Structured {
            provider: Provider::OpenaiCompatible,
            model: ModelConfig::Compatible {
                model: "deepseek-chat".to_string(),
                base_url: "https://api.deepseek.com/v1".to_string(),
                api_key: "sk-test-key".to_string(),
            },
            speed: None,
        };
        let result = resolver.resolve(&model_ref).await;

        let err = result.err().expect("expected an error");
        match err {
            ResolverError::ConstructionFailed { provider, reason } => {
                assert_eq!(provider, Provider::OpenaiCompatible);
                assert!(reason.contains("deepseek-chat"));
                assert!(reason.contains("https://api.deepseek.com/v1"));
            }
            e => panic!("expected ConstructionFailed, got: {e}"),
        }
    }

    #[tokio::test]
    async fn test_resolver_structured_with_speed_hint() {
        let resolver = DefaultModelResolver::new();
        let model_ref = ModelRef::Structured {
            provider: Provider::Gemini,
            model: ModelConfig::Name("gemini-2.5-flash".to_string()),
            speed: Some("fast".to_string()),
        };
        let result = resolver.resolve(&model_ref).await;

        // Speed hint doesn't affect provider resolution
        let err = result.err().expect("expected an error");
        match err {
            ResolverError::ConstructionFailed { provider, .. } => {
                assert_eq!(provider, Provider::Gemini);
            }
            e => panic!("expected ConstructionFailed, got: {e}"),
        }
    }
}
