use serde::{Deserialize, Serialize};

use crate::types::cache_control_ephemeral::CacheControlEphemeral;

/// Parameters for the web fetch tool (version 20250910).
///
/// This tool allows the model to fetch the content of a user-provided URL directly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebFetchTool20250910 {
    /// Name of the tool. This is how the tool will be called by the model and in `tool_use` blocks.
    #[serde(default = "default_name")]
    pub name: String,

    /// If provided, only these domains will be included in results.
    ///
    /// Cannot be used alongside `blocked_domains`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,

    /// If provided, these domains will never appear in results.
    ///
    /// Cannot be used alongside `allowed_domains`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_domains: Option<Vec<String>>,

    /// Create a cache control breakpoint at this content block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControlEphemeral>,

    /// Maximum number of times the tool can be used in the API request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<i32>,

    /// Maximum number of tokens to return from the fetched content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_content_tokens: Option<i32>,
}

fn default_name() -> String {
    "web_fetch".to_string()
}

impl WebFetchTool20250910 {
    /// Creates a new WebFetchTool20250910 instance with default values.
    pub fn new() -> Self {
        Self {
            name: default_name(),
            allowed_domains: None,
            blocked_domains: None,
            cache_control: None,
            max_uses: None,
            max_content_tokens: None,
        }
    }

    /// Sets the allowed domains for the web fetch.
    ///
    /// If provided, only these domains may be fetched.
    /// Cannot be used alongside `blocked_domains`.
    pub fn with_allowed_domains(mut self, domains: Vec<String>) -> Self {
        self.allowed_domains = Some(domains);
        self.blocked_domains = None;
        self
    }

    /// Sets the blocked domains for the web fetch.
    ///
    /// If provided, these domains will never be fetched.
    /// Cannot be used alongside `allowed_domains`.
    pub fn with_blocked_domains(mut self, domains: Vec<String>) -> Self {
        self.blocked_domains = Some(domains);
        self.allowed_domains = None;
        self
    }

    /// Sets the cache control for the web fetch tool.
    pub fn with_cache_control(mut self, cache_control: CacheControlEphemeral) -> Self {
        self.cache_control = Some(cache_control);
        self
    }

    /// Sets the maximum number of times the tool can be used in the API request.
    pub fn with_max_uses(mut self, max_uses: i32) -> Self {
        self.max_uses = Some(max_uses);
        self
    }

    /// Sets the maximum number of tokens to return from the fetched content.
    pub fn with_max_content_tokens(mut self, max_content_tokens: i32) -> Self {
        self.max_content_tokens = Some(max_content_tokens);
        self
    }
}

impl Default for WebFetchTool20250910 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_fetch_tool_serialization() {
        let tool = WebFetchTool20250910::new().with_max_uses(5).with_max_content_tokens(10_000);

        let json = serde_json::to_string(&tool).unwrap();
        let actual: serde_json::Value = serde_json::from_str(&json).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(r#"{"name":"web_fetch","max_content_tokens":10000,"max_uses":5}"#)
                .unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn web_fetch_tool_deserialization() {
        let json = r#"{
            "name": "web_fetch",
            "max_uses": 5,
            "max_content_tokens": 10000
        }"#;

        let tool: WebFetchTool20250910 = serde_json::from_str(json).unwrap();

        assert_eq!(tool.name, "web_fetch");
        assert_eq!(tool.max_uses, Some(5));
        assert_eq!(tool.max_content_tokens, Some(10_000));
    }

    #[test]
    fn allowed_blocked_domains_mutual_exclusivity() {
        let mut tool =
            WebFetchTool20250910::new().with_blocked_domains(vec!["blocked.com".to_string()]);

        assert!(tool.blocked_domains.is_some());
        assert!(tool.allowed_domains.is_none());

        tool = tool.with_allowed_domains(vec!["allowed.com".to_string()]);

        assert!(tool.allowed_domains.is_some());
        assert!(tool.blocked_domains.is_none());

        tool = tool.with_blocked_domains(vec!["blocked.com".to_string()]);

        assert!(tool.blocked_domains.is_some());
        assert!(tool.allowed_domains.is_none());
    }
}
