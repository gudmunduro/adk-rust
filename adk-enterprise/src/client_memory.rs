//! Memory CRUD methods for [`EnterpriseClient`] (beta).
//!
//! Implements Requirements 12.1–12.4:
//! - Memory Store CRUD: `create_memory_store`, `list_memory_stores`, `get_memory_store`,
//!   `delete_memory_store`
//! - Memory CRUD: `create_memory`, `list_memories`, `get_memory`, `update_memory`,
//!   `delete_memory`
//! - Version listing: `list_memory_versions`
//!
//! All methods include the `ADK-Beta: managed-agents-2026-06-01` header.

use reqwest::header::HeaderValue;

use crate::Result;
use crate::client::EnterpriseClient;
use crate::idempotency::IDEMPOTENCY_KEY_HEADER;
use crate::response::{handle_empty_response, handle_response};
use crate::retry::{RetryPolicy, execute_create_with_retry, execute_with_retry};
use crate::types::memory::{
    CreateMemoryParams, CreateMemoryStoreParams, Memory, MemoryStore, MemoryVersion,
    UpdateMemoryParams,
};
use crate::types::pagination::ListResponse;

/// The beta feature header name.
const BETA_HEADER: &str = "ADK-Beta";

/// The beta feature header value for managed agents.
const BETA_HEADER_VALUE: &str = "managed-agents-2026-06-01";

impl EnterpriseClient {
    /// Create a new memory store.
    ///
    /// POSTs to `/memory-stores` with the beta header and an `Idempotency-Key`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_enterprise::{EnterpriseClient, CreateMemoryStoreParams};
    ///
    /// let client = EnterpriseClient::from_env()?;
    /// let store = client.create_memory_store(CreateMemoryStoreParams {
    ///     name: "Agent Memory".into(),
    ///     description: Some("Long-term memory for my assistant".into()),
    /// }).await?;
    /// println!("Created store: {}", store.id);
    /// ```
    pub async fn create_memory_store(
        &self,
        params: CreateMemoryStoreParams,
    ) -> Result<MemoryStore> {
        let url = self.build_url("/memory-stores");
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let body = serde_json::to_vec(&params)?;

        let response = execute_create_with_retry(&policy, |idempotency_key| {
            let url = url.clone();
            let headers = headers.clone();
            let body = body.clone();
            async move {
                reqwest::Client::new()
                    .post(&url)
                    .headers(headers)
                    .header(IDEMPOTENCY_KEY_HEADER, idempotency_key)
                    .body(body)
                    .send()
                    .await
            }
        })
        .await?;

        handle_response(response).await
    }

    /// List all memory stores.
    ///
    /// GETs `/memory-stores` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let stores = client.list_memory_stores().await?;
    /// for store in &stores.data {
    ///     println!("  {} ({})", store.name, store.id);
    /// }
    /// ```
    pub async fn list_memory_stores(&self) -> Result<ListResponse<MemoryStore>> {
        let url = self.build_url("/memory-stores");
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { reqwest::Client::new().get(&url).headers(headers).send().await }
        })
        .await?;

        handle_response(response).await
    }

    /// Retrieve a memory store by ID.
    ///
    /// GETs `/memory-stores/{id}` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let store = client.get_memory_store("mst_abc123").await?;
    /// println!("Store: {}", store.name);
    /// ```
    pub async fn get_memory_store(&self, store_id: &str) -> Result<MemoryStore> {
        let url = self.build_url(&format!("/memory-stores/{store_id}"));
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { reqwest::Client::new().get(&url).headers(headers).send().await }
        })
        .await?;

        handle_response(response).await
    }

    /// Delete a memory store.
    ///
    /// DELETEs `/memory-stores/{id}` with the beta header.
    /// This also deletes all memories within the store.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// client.delete_memory_store("mst_abc123").await?;
    /// ```
    pub async fn delete_memory_store(&self, store_id: &str) -> Result<()> {
        let url = self.build_url(&format!("/memory-stores/{store_id}"));
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { reqwest::Client::new().delete(&url).headers(headers).send().await }
        })
        .await?;

        handle_empty_response(response).await
    }

    /// Create a memory entry within a store.
    ///
    /// POSTs to `/memory-stores/{store_id}/memories` with the beta header and
    /// an `Idempotency-Key`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_enterprise::CreateMemoryParams;
    ///
    /// let memory = client.create_memory(
    ///     "mst_abc123",
    ///     CreateMemoryParams {
    ///         content: "User prefers concise responses.".into(),
    ///         metadata: None,
    ///     },
    /// ).await?;
    /// println!("Created memory: {} (version {})", memory.id, memory.version);
    /// ```
    pub async fn create_memory(
        &self,
        store_id: &str,
        params: CreateMemoryParams,
    ) -> Result<Memory> {
        let url = self.build_url(&format!("/memory-stores/{store_id}/memories"));
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let body = serde_json::to_vec(&params)?;

        let response = execute_create_with_retry(&policy, |idempotency_key| {
            let url = url.clone();
            let headers = headers.clone();
            let body = body.clone();
            async move {
                reqwest::Client::new()
                    .post(&url)
                    .headers(headers)
                    .header(IDEMPOTENCY_KEY_HEADER, idempotency_key)
                    .body(body)
                    .send()
                    .await
            }
        })
        .await?;

        handle_response(response).await
    }

    /// List memories in a store.
    ///
    /// GETs `/memory-stores/{store_id}/memories` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let memories = client.list_memories("mst_abc123").await?;
    /// for mem in &memories.data {
    ///     println!("  [v{}] {}: {}", mem.version, mem.id, mem.content);
    /// }
    /// ```
    pub async fn list_memories(&self, store_id: &str) -> Result<ListResponse<Memory>> {
        let url = self.build_url(&format!("/memory-stores/{store_id}/memories"));
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { reqwest::Client::new().get(&url).headers(headers).send().await }
        })
        .await?;

        handle_response(response).await
    }

    /// Retrieve a specific memory by ID.
    ///
    /// GETs `/memory-stores/{store_id}/memories/{memory_id}` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let memory = client.get_memory("mst_abc123", "mem_xyz").await?;
    /// println!("Content: {}", memory.content);
    /// ```
    pub async fn get_memory(&self, store_id: &str, memory_id: &str) -> Result<Memory> {
        let url = self.build_url(&format!("/memory-stores/{store_id}/memories/{memory_id}"));
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { reqwest::Client::new().get(&url).headers(headers).send().await }
        })
        .await?;

        handle_response(response).await
    }

    /// Update a memory entry.
    ///
    /// PATCHes `/memory-stores/{store_id}/memories/{memory_id}` with the beta header.
    /// Creates a new version of the memory.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_enterprise::UpdateMemoryParams;
    ///
    /// let updated = client.update_memory(
    ///     "mst_abc123",
    ///     "mem_xyz",
    ///     UpdateMemoryParams {
    ///         content: "User prefers detailed technical responses.".into(),
    ///         metadata: None,
    ///     },
    /// ).await?;
    /// println!("Updated to version: {}", updated.version);
    /// ```
    pub async fn update_memory(
        &self,
        store_id: &str,
        memory_id: &str,
        params: UpdateMemoryParams,
    ) -> Result<Memory> {
        let url = self.build_url(&format!("/memory-stores/{store_id}/memories/{memory_id}"));
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let body = serde_json::to_vec(&params)?;

        let response =
            execute_with_retry(&policy, || {
                let url = url.clone();
                let headers = headers.clone();
                let body = body.clone();
                async move {
                    reqwest::Client::new().patch(&url).headers(headers).body(body).send().await
                }
            })
            .await?;

        handle_response(response).await
    }

    /// Delete a memory entry.
    ///
    /// DELETEs `/memory-stores/{store_id}/memories/{memory_id}` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// client.delete_memory("mst_abc123", "mem_xyz").await?;
    /// ```
    pub async fn delete_memory(&self, store_id: &str, memory_id: &str) -> Result<()> {
        let url = self.build_url(&format!("/memory-stores/{store_id}/memories/{memory_id}"));
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { reqwest::Client::new().delete(&url).headers(headers).send().await }
        })
        .await?;

        handle_empty_response(response).await
    }

    /// List version history for a specific memory.
    ///
    /// GETs `/memory-stores/{store_id}/memories/{memory_id}/versions` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let versions = client.list_memory_versions("mst_abc123", "mem_xyz").await?;
    /// for ver in &versions.data {
    ///     println!("  v{}: {} ({})", ver.version, ver.content, ver.created_at);
    /// }
    /// ```
    pub async fn list_memory_versions(
        &self,
        store_id: &str,
        memory_id: &str,
    ) -> Result<ListResponse<MemoryVersion>> {
        let url =
            self.build_url(&format!("/memory-stores/{store_id}/memories/{memory_id}/versions"));
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { reqwest::Client::new().get(&url).headers(headers).send().await }
        })
        .await?;

        handle_response(response).await
    }
}
