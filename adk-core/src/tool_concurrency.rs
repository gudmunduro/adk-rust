//! Semaphore-based concurrency control for parallel tool execution.
//!
//! This module provides [`ToolConcurrencyManager`] which enforces configurable
//! concurrency limits on tool calls, supporting both global limits and per-tool
//! overrides. When limits are reached, the [`BackpressurePolicy`] determines
//! whether excess calls queue or fail immediately.
//!
//! # Architecture
//!
//! The manager holds an optional global semaphore and a map of per-tool semaphores.
//! When a tool call is requested via [`ToolConcurrencyManager::acquire`], the manager
//! checks for a per-tool semaphore first; if none exists, it falls back to the global
//! semaphore. The returned [`ConcurrencyPermit`] is an RAII guard that releases the
//! semaphore on drop.
//!
//! # Example
//!
//! ```rust,ignore
//! use adk_core::{
//!     BackpressurePolicy, ToolConcurrencyConfig, ToolConcurrencyManager,
//! };
//! use std::collections::HashMap;
//!
//! let config = ToolConcurrencyConfig {
//!     max_concurrency: Some(5),
//!     per_tool: HashMap::from([("web_scraper".to_string(), 2)]),
//!     backpressure: BackpressurePolicy::Queue,
//! };
//!
//! let manager = ToolConcurrencyManager::new(&config);
//!
//! // Acquire a permit — blocks if limit reached (Queue policy)
//! let permit = manager.acquire("web_scraper").await.unwrap();
//! // ... execute tool ...
//! drop(permit); // releases the semaphore
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::AdkError;
use crate::context::{BackpressurePolicy, ToolConcurrencyConfig};

/// RAII guard that releases semaphore permits on drop.
///
/// A `ConcurrencyPermit` holds at most one global permit and one per-tool permit.
/// When the permit is dropped, the underlying semaphore slots are released,
/// allowing queued tool calls to proceed.
///
/// # Example
///
/// ```rust,ignore
/// use adk_core::{ToolConcurrencyConfig, ToolConcurrencyManager};
///
/// let config = ToolConcurrencyConfig {
///     max_concurrency: Some(3),
///     ..Default::default()
/// };
/// let manager = ToolConcurrencyManager::new(&config);
///
/// let permit = manager.acquire("my_tool").await.unwrap();
/// // Tool executes while permit is held...
/// drop(permit); // Semaphore slot released
/// ```
pub struct ConcurrencyPermit {
    _global: Option<tokio::sync::OwnedSemaphorePermit>,
    _per_tool: Option<tokio::sync::OwnedSemaphorePermit>,
}

/// Manages semaphores for tool concurrency enforcement.
///
/// Created from a [`ToolConcurrencyConfig`], the manager pre-allocates semaphores
/// for the global limit and each per-tool override. Use [`acquire`](Self::acquire)
/// to obtain a [`ConcurrencyPermit`] before executing a tool.
///
/// # Example
///
/// ```rust,ignore
/// use adk_core::{
///     BackpressurePolicy, ToolConcurrencyConfig, ToolConcurrencyManager,
/// };
/// use std::collections::HashMap;
///
/// let config = ToolConcurrencyConfig {
///     max_concurrency: Some(5),
///     per_tool: HashMap::from([("expensive_tool".to_string(), 1)]),
///     backpressure: BackpressurePolicy::Queue,
/// };
///
/// let manager = ToolConcurrencyManager::new(&config);
///
/// // Only 1 "expensive_tool" can run at a time
/// let permit = manager.acquire("expensive_tool").await.unwrap();
/// // ... run tool ...
/// drop(permit);
///
/// // Other tools use the global limit of 5
/// let permit = manager.acquire("cheap_tool").await.unwrap();
/// drop(permit);
/// ```
pub struct ToolConcurrencyManager {
    global_semaphore: Option<Arc<Semaphore>>,
    per_tool_semaphores: HashMap<String, Arc<Semaphore>>,
    backpressure: BackpressurePolicy,
}

impl ToolConcurrencyManager {
    /// Create a new manager from the given configuration.
    ///
    /// Allocates semaphores based on the config:
    /// - A global semaphore with `max_concurrency` permits (if set)
    /// - Per-tool semaphores for each entry in `per_tool`
    ///
    /// # Example
    ///
    /// ```rust
    /// use adk_core::{ToolConcurrencyConfig, ToolConcurrencyManager};
    ///
    /// let config = ToolConcurrencyConfig {
    ///     max_concurrency: Some(10),
    ///     ..Default::default()
    /// };
    /// let manager = ToolConcurrencyManager::new(&config);
    /// ```
    pub fn new(config: &ToolConcurrencyConfig) -> Self {
        let global_semaphore = config.max_concurrency.map(|n| Arc::new(Semaphore::new(n)));

        let per_tool_semaphores = config
            .per_tool
            .iter()
            .map(|(name, &limit)| (name.clone(), Arc::new(Semaphore::new(limit))))
            .collect();

        Self { global_semaphore, per_tool_semaphores, backpressure: config.backpressure.clone() }
    }

    /// Returns `true` if this manager has any concurrency limits configured.
    ///
    /// When no limits are configured (no global limit and no per-tool overrides),
    /// calling [`acquire`](Self::acquire) always succeeds immediately with no
    /// semaphore enforcement.
    pub fn has_limits(&self) -> bool {
        self.global_semaphore.is_some() || !self.per_tool_semaphores.is_empty()
    }

    /// Acquire a permit for the named tool.
    ///
    /// If a per-tool override exists for `tool_name`, the per-tool semaphore is used.
    /// Otherwise, the global semaphore is used (if configured). When neither a per-tool
    /// override nor a global limit is configured, a permit is returned immediately with
    /// no semaphore enforcement.
    ///
    /// # Errors
    ///
    /// Returns `AdkError` when [`BackpressurePolicy::Fail`] is configured and no
    /// permit is immediately available.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_core::{
    ///     BackpressurePolicy, ToolConcurrencyConfig, ToolConcurrencyManager,
    /// };
    ///
    /// let config = ToolConcurrencyConfig {
    ///     max_concurrency: Some(1),
    ///     backpressure: BackpressurePolicy::Fail,
    ///     ..Default::default()
    /// };
    /// let manager = ToolConcurrencyManager::new(&config);
    ///
    /// // First acquire succeeds
    /// let permit1 = manager.acquire("tool_a").await.unwrap();
    ///
    /// // Second acquire fails immediately (Fail policy)
    /// let result = manager.acquire("tool_b").await;
    /// assert!(result.is_err());
    ///
    /// drop(permit1);
    /// ```
    pub async fn acquire(&self, tool_name: &str) -> Result<ConcurrencyPermit, AdkError> {
        // Determine which semaphore to use: per-tool takes precedence
        let has_per_tool = self.per_tool_semaphores.contains_key(tool_name);

        let per_tool_permit = if has_per_tool {
            let sem = self.per_tool_semaphores[tool_name].clone();
            Some(self.acquire_permit(sem, tool_name).await?)
        } else {
            None
        };

        // If there's no per-tool override, use the global semaphore
        let global_permit = if !has_per_tool {
            match &self.global_semaphore {
                Some(sem) => Some(self.acquire_permit(sem.clone(), tool_name).await?),
                None => None,
            }
        } else {
            None
        };

        Ok(ConcurrencyPermit { _global: global_permit, _per_tool: per_tool_permit })
    }

    /// Acquire a single permit from the given semaphore, respecting backpressure policy.
    async fn acquire_permit(
        &self,
        semaphore: Arc<Semaphore>,
        tool_name: &str,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, AdkError> {
        match self.backpressure {
            BackpressurePolicy::Queue => semaphore
                .acquire_owned()
                .await
                .map_err(|_| AdkError::tool(format!("concurrency semaphore closed: {tool_name}"))),
            BackpressurePolicy::Fail => semaphore
                .try_acquire_owned()
                .map_err(|_| AdkError::tool(format!("concurrency limit reached: {tool_name}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unlimited_concurrency() {
        let config = ToolConcurrencyConfig::default();
        let manager = ToolConcurrencyManager::new(&config);

        assert!(!manager.has_limits());
        let permit = manager.acquire("any_tool").await;
        assert!(permit.is_ok());
    }

    #[tokio::test]
    async fn test_global_limit_queue_policy() {
        let config = ToolConcurrencyConfig {
            max_concurrency: Some(2),
            backpressure: BackpressurePolicy::Queue,
            ..Default::default()
        };
        let manager = ToolConcurrencyManager::new(&config);

        assert!(manager.has_limits());
        let _p1 = manager.acquire("tool_a").await.unwrap();
        let _p2 = manager.acquire("tool_b").await.unwrap();
    }

    #[tokio::test]
    async fn test_global_limit_fail_policy() {
        let config = ToolConcurrencyConfig {
            max_concurrency: Some(1),
            backpressure: BackpressurePolicy::Fail,
            ..Default::default()
        };
        let manager = ToolConcurrencyManager::new(&config);

        let _p1 = manager.acquire("tool_a").await.unwrap();
        let result = manager.acquire("tool_b").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_per_tool_override() {
        let config = ToolConcurrencyConfig {
            max_concurrency: Some(10),
            per_tool: HashMap::from([("limited_tool".to_string(), 1)]),
            backpressure: BackpressurePolicy::Fail,
        };
        let manager = ToolConcurrencyManager::new(&config);

        // Per-tool limit of 1
        let _p1 = manager.acquire("limited_tool").await.unwrap();
        let result = manager.acquire("limited_tool").await;
        assert!(result.is_err());

        // Other tools use global limit of 10
        let _p2 = manager.acquire("other_tool").await.unwrap();
        assert!(_p2._global.is_some());
    }

    #[tokio::test]
    async fn test_permit_release_on_drop() {
        let config = ToolConcurrencyConfig {
            max_concurrency: Some(1),
            backpressure: BackpressurePolicy::Fail,
            ..Default::default()
        };
        let manager = ToolConcurrencyManager::new(&config);

        let permit = manager.acquire("tool").await.unwrap();
        drop(permit);

        // After drop, we can acquire again
        let result = manager.acquire("tool").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_per_tool_permit_release_on_drop() {
        let config = ToolConcurrencyConfig {
            per_tool: HashMap::from([("special".to_string(), 1)]),
            backpressure: BackpressurePolicy::Fail,
            ..Default::default()
        };
        let manager = ToolConcurrencyManager::new(&config);

        let permit = manager.acquire("special").await.unwrap();
        drop(permit);

        // After drop, we can acquire again
        let result = manager.acquire("special").await;
        assert!(result.is_ok());
    }
}
