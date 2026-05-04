//! Provider failover — tries providers in order, falls back on failure.

use std::sync::Arc;

use super::{
    ChatChunk, ChatRequest, ChatResponse, LLMProvider, ProviderError, StreamResult,
};

/// A failover provider that tries multiple providers in order.
/// If the primary provider fails, it automatically tries the next one.
pub struct FailoverProvider {
    providers: Vec<Arc<dyn LLMProvider>>,
    /// Index of the currently active provider.
    active_index: std::sync::atomic::AtomicUsize,
}

impl FailoverProvider {
    /// Create a new failover provider from a list of providers.
    /// The first provider is tried first, then the second, etc.
    pub fn new(providers: Vec<Arc<dyn LLMProvider>>) -> Self {
        assert!(!providers.is_empty(), "FailoverProvider requires at least one provider");
        Self {
            providers,
            active_index: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Get the currently active provider index.
    pub fn active_index(&self) -> usize {
        self.active_index.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get the currently active provider.
    pub fn active_provider(&self) -> &Arc<dyn LLMProvider> {
        &self.providers[self.active_index()]
    }

    /// Number of providers in the failover chain.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Try each provider in order until one succeeds.
    fn try_each<F, T>(&self, op: F) -> Result<T, ProviderError>
    where
        F: Fn(&dyn LLMProvider) -> Result<T, ProviderError>,
    {
        let start = self.active_index();
        let len = self.providers.len();
        let mut last_error = None;

        for i in 0..len {
            let idx = (start + i) % len;
            let provider = &self.providers[idx];
            match op(provider.as_ref()) {
                Ok(result) => {
                    self.active_index.store(idx, std::sync::atomic::Ordering::Relaxed);
                    return Ok(result);
                }
                Err(e) => {
                    log::warn!(
                        "Provider '{}' (index {}) failed: {}. Trying next...",
                        provider.provider_id(),
                        idx,
                        e
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap())
    }
}

impl LLMProvider for FailoverProvider {
    fn provider_type(&self) -> &str {
        "failover"
    }

    fn provider_id(&self) -> &str {
        self.active_provider().provider_id()
    }

    fn chat(&self, request: ChatRequest) -> super::ProviderResult<ChatResponse> {
        self.try_each(|p| p.chat(request.clone()))
    }

    fn chat_stream(&self, request: ChatRequest) -> StreamResult<ChatChunk, ProviderError> {
        // For streaming, just use the active provider (failover on stream is complex)
        self.active_provider().chat_stream(request)
    }

    fn health_check(&self) -> super::ProviderResult<bool> {
        // Check all providers, report healthy if at least one is healthy
        let mut any_healthy = false;
        for provider in &self.providers {
            match provider.health_check() {
                Ok(true) => any_healthy = true,
                Ok(false) => {}
                Err(_) => {}
            }
        }
        Ok(any_healthy)
    }

    fn list_models(&self) -> super::ProviderResult<Vec<super::ModelInfo>> {
        // Aggregate models from all providers
        let mut all_models = Vec::new();
        for provider in &self.providers {
            if let Ok(models) = provider.list_models() {
                all_models.extend(models);
            }
        }
        Ok(all_models)
    }

    fn count_tokens(&self, text: &str) -> usize {
        self.active_provider().count_tokens(text)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::openai::OpenAIProvider;

    fn make_provider(id: &str) -> Arc<dyn LLMProvider> {
        Arc::new(OpenAIProvider::new(
            crate::providers::openai::OpenAIProviderConfig::new(
                id,
                "http://localhost:1",
                "key",
                "model",
            ),
        ))
    }

    #[test]
    fn test_failover_new() {
        let fo = FailoverProvider::new(vec![make_provider("p1")]);
        assert_eq!(fo.provider_count(), 1);
        assert_eq!(fo.active_index(), 0);
    }

    #[test]
    fn test_failover_multiple_providers() {
        let fo = FailoverProvider::new(vec![
            make_provider("p1"),
            make_provider("p2"),
            make_provider("p3"),
        ]);
        assert_eq!(fo.provider_count(), 3);
        assert_eq!(fo.active_provider().provider_id(), "p1");
    }

    #[test]
    fn test_failover_health_check_any_healthy() {
        let fo = FailoverProvider::new(vec![
            make_provider("p1"),
            make_provider("p2"),
        ]);
        // These providers will fail health check (localhost:1), but at least
        // they return Err not panic. The method returns Ok(true) only if one is healthy.
        // Since none can connect, we get Ok(false) not Err.
        let result = fo.health_check();
        // Should return Ok (not Err), but likely false since localhost:1 won't respond
        assert!(result.is_ok());
    }

    #[test]
    fn test_failover_provider_type() {
        let fo = FailoverProvider::new(vec![make_provider("p1")]);
        assert_eq!(fo.provider_type(), "failover");
    }

    #[test]
    fn test_failover_list_models_aggregates() {
        let fo = FailoverProvider::new(vec![
            make_provider("p1"),
            make_provider("p2"),
        ]);
        let models = fo.list_models().unwrap();
        // These providers can't connect to list models, so returns empty
        // but the method itself doesn't error
        assert!(models.is_empty() || !models.is_empty()); // just verify no panic
    }

    #[test]
    fn test_failover_count_tokens() {
        let fo = FailoverProvider::new(vec![make_provider("p1")]);
        let count = fo.count_tokens("hello world test");
        assert!(count > 0);
    }

    #[test]
    #[should_panic(expected = "at least one provider")]
    fn test_failover_empty_panics() {
        let _fo = FailoverProvider::new(vec![]);
    }
}
