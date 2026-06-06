//! Environment CRUD methods for `EnterpriseClient`.
//!
//! Implements Requirements 3.1–3.5:
//! - Create environment (POST /environments)
//! - Get environment (GET /environments/{id})
//! - Archive environment (POST /environments/{id}/archive)
//! - Delete environment (DELETE /environments/{id})
//! - Download environment snapshot (GET /environments/{id}/download → raw tar bytes)

use crate::client::EnterpriseClient;
use crate::idempotency::IDEMPOTENCY_KEY_HEADER;
use crate::response::{handle_empty_response, handle_response};
use crate::retry::{RetryPolicy, execute_create_with_retry, execute_with_retry};
use crate::types::{CreateEnvironmentParams, Environment};
use crate::{EnterpriseError, Result};

impl EnterpriseClient {
    /// Create a new execution environment.
    ///
    /// POSTs to `/environments` with an auto-generated `Idempotency-Key` header.
    /// The server returns the created environment with its assigned `id` and timestamps.
    ///
    /// # Arguments
    ///
    /// * `params` - The environment configuration parameters.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_enterprise::{EnterpriseClient, CreateEnvironmentParams};
    ///
    /// let client = EnterpriseClient::from_env()?;
    /// let env = client.create_environment(CreateEnvironmentParams::cloud("my-sandbox")).await?;
    /// println!("Created environment: {}", env.id);
    /// ```
    pub async fn create_environment(&self, params: CreateEnvironmentParams) -> Result<Environment> {
        let url = self.build_url("/environments");
        let headers = self.default_headers();
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let body = serde_json::to_vec(&params)?;

        let response = execute_create_with_retry(&policy, |idempotency_key| {
            let url = url.clone();
            let headers = headers.clone();
            let body = body.clone();
            async move {
                self.http
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

    /// Get an environment by its ID.
    ///
    /// GETs `/environments/{id}` and returns the environment details.
    ///
    /// # Arguments
    ///
    /// * `env_id` - The environment identifier (e.g., `"env_abc123"`).
    ///
    /// # Errors
    ///
    /// Returns `EnterpriseError::NotFound` if the environment does not exist.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let env = client.get_environment("env_abc123").await?;
    /// println!("Environment: {} ({})", env.name, env.id);
    /// ```
    pub async fn get_environment(&self, env_id: &str) -> Result<Environment> {
        let url = self.build_url(&format!("/environments/{env_id}"));
        let headers = self.default_headers();
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { self.http.get(&url).headers(headers).send().await }
        })
        .await?;

        handle_response(response).await
    }

    /// Archive an environment.
    ///
    /// POSTs to `/environments/{id}/archive`. Archived environments cannot be used
    /// for new sessions but retain their data for auditing.
    ///
    /// # Arguments
    ///
    /// * `env_id` - The environment identifier.
    ///
    /// # Returns
    ///
    /// The updated environment with `archived_at` set.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let env = client.archive_environment("env_abc123").await?;
    /// assert!(env.archived_at.is_some());
    /// ```
    pub async fn archive_environment(&self, env_id: &str) -> Result<Environment> {
        let url = self.build_url(&format!("/environments/{env_id}/archive"));
        let headers = self.default_headers();
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { self.http.post(&url).headers(headers).send().await }
        })
        .await?;

        handle_response(response).await
    }

    /// Delete an environment permanently.
    ///
    /// DELETEs `/environments/{id}`. This operation cannot be undone.
    ///
    /// # Arguments
    ///
    /// * `env_id` - The environment identifier.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// client.delete_environment("env_abc123").await?;
    /// ```
    pub async fn delete_environment(&self, env_id: &str) -> Result<()> {
        let url = self.build_url(&format!("/environments/{env_id}"));
        let headers = self.default_headers();
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { self.http.delete(&url).headers(headers).send().await }
        })
        .await?;

        handle_empty_response(response).await
    }

    /// Download an environment snapshot as a tar archive.
    ///
    /// GETs `/environments/{id}/download` and returns the raw bytes of the
    /// tar archive. This can be written to disk or extracted programmatically.
    ///
    /// # Arguments
    ///
    /// * `env_id` - The environment identifier.
    ///
    /// # Returns
    ///
    /// Raw bytes of the tar snapshot.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tar_bytes = client.download_environment("env_abc123").await?;
    /// std::fs::write("environment.tar", &tar_bytes)?;
    /// ```
    pub async fn download_environment(&self, env_id: &str) -> Result<Vec<u8>> {
        let url = self.build_url(&format!("/environments/{env_id}/download"));
        let headers = self.default_headers();
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { self.http.get(&url).headers(headers).send().await }
        })
        .await?;

        let status = response.status();
        if status.is_success() {
            let bytes = response.bytes().await.map_err(EnterpriseError::Connection)?;
            Ok(bytes.to_vec())
        } else {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(std::time::Duration::from_secs);
            let body = response.text().await.unwrap_or_default();
            Err(crate::response::map_api_error(status, &body, retry_after))
        }
    }
}
