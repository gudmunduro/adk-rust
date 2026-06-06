//! Vault and Credential CRUD methods for [`EnterpriseClient`] (beta).
//!
//! Implements Requirements 11.1–11.4:
//! - Vault CRUD: `create_vault`, `list_vaults`, `get_vault`, `archive_vault`, `delete_vault`
//! - Credential CRUD: `create_credential`, `list_credentials`, `update_credential`,
//!   `validate_credential`, `delete_credential`
//!
//! All methods include the `ADK-Beta: managed-agents-2026-06-01` header.

use reqwest::header::HeaderValue;

use crate::Result;
use crate::client::EnterpriseClient;
use crate::idempotency::IDEMPOTENCY_KEY_HEADER;
use crate::response::{handle_empty_response, handle_response};
use crate::retry::{RetryPolicy, execute_create_with_retry, execute_with_retry};
use crate::types::pagination::ListResponse;
use crate::types::vault::{
    CreateCredentialParams, CreateVaultParams, Credential, CredentialValidation,
    UpdateCredentialParams, Vault,
};

/// The beta feature header name.
const BETA_HEADER: &str = "ADK-Beta";

/// The beta feature header value for managed agents.
const BETA_HEADER_VALUE: &str = "managed-agents-2026-06-01";

impl EnterpriseClient {
    /// Create a new vault.
    ///
    /// POSTs to `/vaults` with the beta header and an `Idempotency-Key`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_enterprise::{EnterpriseClient, CreateVaultParams};
    ///
    /// let client = EnterpriseClient::from_env()?;
    /// let vault = client.create_vault(CreateVaultParams {
    ///     name: "My Vault".into(),
    ///     description: Some("MCP credentials".into()),
    /// }).await?;
    /// println!("Created vault: {}", vault.id);
    /// ```
    pub async fn create_vault(&self, params: CreateVaultParams) -> Result<Vault> {
        let url = self.build_url("/vaults");
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

    /// List all vaults.
    ///
    /// GETs `/vaults` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let vaults = client.list_vaults().await?;
    /// for vault in &vaults.data {
    ///     println!("  {} ({})", vault.name, vault.id);
    /// }
    /// ```
    pub async fn list_vaults(&self) -> Result<ListResponse<Vault>> {
        let url = self.build_url("/vaults");
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

    /// Retrieve a vault by ID.
    ///
    /// GETs `/vaults/{id}` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let vault = client.get_vault("vlt_abc123").await?;
    /// println!("Vault: {}", vault.name);
    /// ```
    pub async fn get_vault(&self, vault_id: &str) -> Result<Vault> {
        let url = self.build_url(&format!("/vaults/{vault_id}"));
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

    /// Archive a vault.
    ///
    /// POSTs to `/vaults/{id}/archive` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// client.archive_vault("vlt_abc123").await?;
    /// ```
    pub async fn archive_vault(&self, vault_id: &str) -> Result<()> {
        let url = self.build_url(&format!("/vaults/{vault_id}/archive"));
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { reqwest::Client::new().post(&url).headers(headers).send().await }
        })
        .await?;

        handle_empty_response(response).await
    }

    /// Permanently delete a vault.
    ///
    /// DELETEs `/vaults/{id}` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// client.delete_vault("vlt_abc123").await?;
    /// ```
    pub async fn delete_vault(&self, vault_id: &str) -> Result<()> {
        let url = self.build_url(&format!("/vaults/{vault_id}"));
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

    /// Create a credential within a vault.
    ///
    /// POSTs to `/vaults/{vault_id}/credentials` with the beta header and
    /// an `Idempotency-Key`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_enterprise::CreateCredentialParams;
    ///
    /// let cred = client.create_credential(
    ///     "vlt_abc123",
    ///     CreateCredentialParams::static_bearer("My API", "https://api.example.com", "sk-token"),
    /// ).await?;
    /// println!("Created credential: {}", cred.id);
    /// ```
    pub async fn create_credential(
        &self,
        vault_id: &str,
        params: CreateCredentialParams,
    ) -> Result<Credential> {
        let url = self.build_url(&format!("/vaults/{vault_id}/credentials"));
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

    /// List credentials in a vault.
    ///
    /// GETs `/vaults/{vault_id}/credentials` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let creds = client.list_credentials("vlt_abc123").await?;
    /// for cred in &creds.data {
    ///     println!("  {} ({}) - {}", cred.name, cred.id, cred.credential_type);
    /// }
    /// ```
    pub async fn list_credentials(&self, vault_id: &str) -> Result<ListResponse<Credential>> {
        let url = self.build_url(&format!("/vaults/{vault_id}/credentials"));
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

    /// Update a credential in a vault.
    ///
    /// PATCHes `/vaults/{vault_id}/credentials/{cred_id}` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use adk_enterprise::UpdateCredentialParams;
    ///
    /// let updated = client.update_credential(
    ///     "vlt_abc123",
    ///     "cred_xyz",
    ///     UpdateCredentialParams {
    ///         token: Some("new-token".into()),
    ///         ..Default::default()
    ///     },
    /// ).await?;
    /// ```
    pub async fn update_credential(
        &self,
        vault_id: &str,
        cred_id: &str,
        params: UpdateCredentialParams,
    ) -> Result<Credential> {
        let url = self.build_url(&format!("/vaults/{vault_id}/credentials/{cred_id}"));
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

    /// Validate a credential.
    ///
    /// POSTs to `/vaults/{vault_id}/credentials/{cred_id}/validate` with the beta header.
    /// Returns a `CredentialValidation` indicating whether the credential is still valid.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let validation = client.validate_credential("vlt_abc123", "cred_xyz").await?;
    /// if validation.valid {
    ///     println!("Credential is valid");
    /// } else {
    ///     println!("Credential invalid: {:?}", validation.message);
    /// }
    /// ```
    pub async fn validate_credential(
        &self,
        vault_id: &str,
        cred_id: &str,
    ) -> Result<CredentialValidation> {
        let url = self.build_url(&format!("/vaults/{vault_id}/credentials/{cred_id}/validate"));
        let mut headers = self.default_headers();
        headers.insert(BETA_HEADER, HeaderValue::from_static(BETA_HEADER_VALUE));
        let policy = RetryPolicy::from_config(self.config.max_retries, self.config.retry_backoff);

        let response = execute_with_retry(&policy, || {
            let url = url.clone();
            let headers = headers.clone();
            async move { reqwest::Client::new().post(&url).headers(headers).send().await }
        })
        .await?;

        handle_response(response).await
    }

    /// Delete a credential from a vault.
    ///
    /// DELETEs `/vaults/{vault_id}/credentials/{cred_id}` with the beta header.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// client.delete_credential("vlt_abc123", "cred_xyz").await?;
    /// ```
    pub async fn delete_credential(&self, vault_id: &str, cred_id: &str) -> Result<()> {
        let url = self.build_url(&format!("/vaults/{vault_id}/credentials/{cred_id}"));
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
}
