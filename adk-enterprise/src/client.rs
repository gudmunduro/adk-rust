//! EnterpriseClient — the primary entry point for all API operations.

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

use crate::Result;
use crate::config::ClientConfig;

/// The custom version header name for the ADK Managed Agent Service.
const VERSION_HEADER: &str = "ADK-Managed-Agent";

/// Client for the ADK-Rust Enterprise Managed Agent Service.
///
/// This is the primary entry point for all API operations. It handles
/// authentication, request formatting, response parsing, SSE streaming,
/// and automatic retry with exponential backoff.
///
/// The client is `Clone` and safe to share across tasks without `Arc` wrapping.
///
/// # Example
///
/// ```rust,ignore
/// use adk_enterprise::EnterpriseClient;
///
/// // From an explicit API key
/// let client = EnterpriseClient::new("adk_live_...")?;
///
/// // From environment variable (ADK_API_KEY or ADK_ENTERPRISE_KEY)
/// let client = EnterpriseClient::from_env()?;
///
/// // Targeting a self-hosted deployment
/// let client = EnterpriseClient::self_hosted(
///     "adk_live_...",
///     "https://my-server.internal/managed/v1",
/// )?;
/// ```
#[derive(Clone, Debug)]
pub struct EnterpriseClient {
    /// The underlying HTTP client.
    pub(crate) http: reqwest::Client,
    /// Client configuration (API key, base URL, timeouts, etc.).
    pub(crate) config: ClientConfig,
}

impl EnterpriseClient {
    /// Create a new client targeting the production URL.
    ///
    /// # Arguments
    ///
    /// * `api_key` - Your API key (e.g., `adk_live_...`)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let client = EnterpriseClient::new("adk_live_abc123")?;
    /// ```
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let config = ClientConfig::new(api_key);
        Self::with_config(config)
    }

    /// Create a client by reading the API key from environment variables.
    ///
    /// Checks `ADK_API_KEY` first, then `ADK_ENTERPRISE_KEY`.
    ///
    /// # Errors
    ///
    /// Returns `EnterpriseError::Authentication` if neither environment variable is set.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ADK_API_KEY")
            .or_else(|_| std::env::var("ADK_ENTERPRISE_KEY"))
            .map_err(|_| crate::EnterpriseError::Authentication {
                message:
                    "No API key found. Set ADK_API_KEY or ADK_ENTERPRISE_KEY environment variable."
                        .into(),
            })?;
        Self::new(api_key)
    }

    /// Create a client targeting a self-hosted deployment.
    ///
    /// # Arguments
    ///
    /// * `api_key` - Your API key
    /// * `base_url` - The base URL of your self-hosted deployment
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let client = EnterpriseClient::self_hosted(
    ///     "adk_live_abc123",
    ///     "https://my-server.internal/managed/v1",
    /// )?;
    /// ```
    pub fn self_hosted(api_key: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        let config = ClientConfig::self_hosted(api_key, base_url);
        Self::with_config(config)
    }

    /// Create a client with full configuration control.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    /// use adk_enterprise::{ClientConfig, EnterpriseClient};
    ///
    /// let config = ClientConfig::new("adk_live_abc123")
    ///     .with_base_url("https://custom.example.com/managed/v1")
    ///     .with_sse_timeout(Duration::from_secs(600))
    ///     .with_max_retries(5);
    ///
    /// let client = EnterpriseClient::with_config(config)?;
    /// ```
    pub fn with_config(config: ClientConfig) -> Result<Self> {
        let http = reqwest::Client::new();
        Ok(Self { http, config })
    }

    /// Build a full URL by appending the endpoint path to the base URL.
    ///
    /// Handles trailing/leading slashes to avoid double-slash issues.
    pub(crate) fn build_url(&self, endpoint: &str) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        let path = endpoint.trim_start_matches('/');
        format!("{base}/{path}")
    }

    /// Build the default headers included on every request:
    ///
    /// - `Authorization: Bearer {api_key}`
    /// - `ADK-Managed-Agent: {version}` (e.g., `2026-06-01`)
    /// - `Content-Type: application/json`
    pub(crate) fn default_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();

        // Authorization: Bearer <api_key>
        let auth_value = format!("Bearer {}", self.config.api_key);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value).expect("API key contains invalid header characters"),
        );

        // ADK-Managed-Agent: <version>
        headers.insert(
            VERSION_HEADER,
            HeaderValue::from_str(&self.config.version)
                .expect("version string contains invalid header characters"),
        );

        // Content-Type: application/json
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        headers
    }

    /// Get a reference to the client configuration.
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_BASE_URL;

    #[test]
    fn test_new_creates_client_with_production_url() {
        let client = EnterpriseClient::new("adk_live_test123").unwrap();
        assert_eq!(client.config.base_url, DEFAULT_BASE_URL);
        assert_eq!(client.config.api_key, "adk_live_test123");
    }

    #[test]
    fn test_self_hosted_creates_client_with_custom_url() {
        let client =
            EnterpriseClient::self_hosted("adk_live_key", "https://custom.example.com/managed/v1")
                .unwrap();
        assert_eq!(client.config.base_url, "https://custom.example.com/managed/v1");
        assert_eq!(client.config.api_key, "adk_live_key");
    }

    #[test]
    fn test_with_config_uses_provided_config() {
        let config = ClientConfig::new("my_key")
            .with_base_url("https://my-server.com/v1")
            .with_max_retries(5);
        let client = EnterpriseClient::with_config(config).unwrap();
        assert_eq!(client.config.base_url, "https://my-server.com/v1");
        assert_eq!(client.config.max_retries, 5);
    }

    #[test]
    fn test_build_url_basic() {
        let client = EnterpriseClient::new("key").unwrap();
        let url = client.build_url("/agents");
        assert_eq!(url, format!("{DEFAULT_BASE_URL}/agents"));
    }

    #[test]
    fn test_build_url_no_double_slash() {
        let client = EnterpriseClient::self_hosted("key", "https://example.com/v1/").unwrap();
        let url = client.build_url("/agents");
        assert_eq!(url, "https://example.com/v1/agents");
    }

    #[test]
    fn test_build_url_no_leading_slash_in_endpoint() {
        let client = EnterpriseClient::new("key").unwrap();
        let url = client.build_url("agents/agt_123");
        assert_eq!(url, format!("{DEFAULT_BASE_URL}/agents/agt_123"));
    }

    #[test]
    fn test_default_headers_contains_auth() {
        let client = EnterpriseClient::new("adk_live_secret").unwrap();
        let headers = client.default_headers();
        let auth = headers.get(AUTHORIZATION).unwrap().to_str().unwrap();
        assert_eq!(auth, "Bearer adk_live_secret");
    }

    #[test]
    fn test_default_headers_contains_version() {
        let client = EnterpriseClient::new("key").unwrap();
        let headers = client.default_headers();
        let version = headers.get(VERSION_HEADER).unwrap().to_str().unwrap();
        assert_eq!(version, "2026-06-01");
    }

    #[test]
    fn test_default_headers_contains_content_type() {
        let client = EnterpriseClient::new("key").unwrap();
        let headers = client.default_headers();
        let ct = headers.get(CONTENT_TYPE).unwrap().to_str().unwrap();
        assert_eq!(ct, "application/json");
    }

    #[test]
    fn test_client_is_clone() {
        let client = EnterpriseClient::new("key").unwrap();
        let cloned = client.clone();
        assert_eq!(cloned.config.api_key, "key");
    }

    /// Tests for `from_env()` are combined into a single test function
    /// to avoid race conditions from parallel env var manipulation.
    #[test]
    fn test_from_env_variants() {
        // Save original values
        let saved_api = std::env::var("ADK_API_KEY").ok();
        let saved_ent = std::env::var("ADK_ENTERPRISE_KEY").ok();

        // SAFETY: env manipulation in test.
        unsafe {
            // Test 1: Missing both vars returns an error
            std::env::remove_var("ADK_API_KEY");
            std::env::remove_var("ADK_ENTERPRISE_KEY");
        }
        let result = EnterpriseClient::from_env();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, crate::EnterpriseError::Authentication { .. }));

        unsafe {
            // Test 2: ADK_API_KEY is preferred
            std::env::set_var("ADK_API_KEY", "adk_live_from_env");
        }
        let client = EnterpriseClient::from_env().unwrap();
        assert_eq!(client.config.api_key, "adk_live_from_env");

        unsafe {
            // Test 3: Falls back to ADK_ENTERPRISE_KEY when ADK_API_KEY is unset
            std::env::remove_var("ADK_API_KEY");
            std::env::set_var("ADK_ENTERPRISE_KEY", "adk_live_enterprise");
        }
        let client = EnterpriseClient::from_env().unwrap();
        assert_eq!(client.config.api_key, "adk_live_enterprise");

        // Restore original values
        unsafe {
            std::env::remove_var("ADK_API_KEY");
            std::env::remove_var("ADK_ENTERPRISE_KEY");
            if let Some(val) = saved_api {
                std::env::set_var("ADK_API_KEY", val);
            }
            if let Some(val) = saved_ent {
                std::env::set_var("ADK_ENTERPRISE_KEY", val);
            }
        }
    }

    #[test]
    fn test_config_accessor() {
        let client = EnterpriseClient::new("my_key").unwrap();
        assert_eq!(client.config().api_key, "my_key");
        assert_eq!(client.config().version, "2026-06-01");
    }
}
