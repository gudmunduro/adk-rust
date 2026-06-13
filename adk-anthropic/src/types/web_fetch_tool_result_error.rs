use serde::{Deserialize, Serialize};
use std::fmt;

/// Error codes that can be returned when a web fetch tool operation fails.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebFetchErrorCode {
    /// The input provided to the web fetch tool is invalid.
    InvalidToolInput,

    /// The web fetch service is currently unavailable.
    Unavailable,

    /// The maximum number of uses for the web fetch tool has been exceeded.
    MaxUsesExceeded,

    /// Too many requests have been made to the web fetch service.
    TooManyRequests,

    /// The requested URL is not in the allowed domains list.
    UrlNotAllowed,

    /// The fetch operation failed.
    FetchFailed,

    /// The requested URL was not mentioned in the prior context (Anthropic only permits fetching
    /// URLs that appeared in earlier web search results within the same context window).
    UrlNotInPriorContext,

    /// An unrecognised error code from a future API version. Stored as a string for
    /// forward-compatibility.
    #[serde(other)]
    Unknown,
}

impl fmt::Display for WebFetchErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WebFetchErrorCode::InvalidToolInput => write!(f, "invalid_tool_input"),
            WebFetchErrorCode::Unavailable => write!(f, "unavailable"),
            WebFetchErrorCode::MaxUsesExceeded => write!(f, "max_uses_exceeded"),
            WebFetchErrorCode::TooManyRequests => write!(f, "too_many_requests"),
            WebFetchErrorCode::UrlNotAllowed => write!(f, "url_not_allowed"),
            WebFetchErrorCode::FetchFailed => write!(f, "fetch_failed"),
            WebFetchErrorCode::UrlNotInPriorContext => write!(f, "url_not_in_prior_context"),
            WebFetchErrorCode::Unknown => write!(f, "unknown"),
        }
    }
}

/// An error that occurred when using the web fetch tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebFetchToolResultError {
    /// The specific error code indicating the type of failure.
    pub error_code: WebFetchErrorCode,
}

impl WebFetchToolResultError {
    /// Creates a new WebFetchToolResultError with the specified error code.
    pub fn new(error_code: WebFetchErrorCode) -> Self {
        Self { error_code }
    }

    /// Returns true if the error is due to an invalid tool input.
    pub fn is_invalid_input(&self) -> bool {
        matches!(self.error_code, WebFetchErrorCode::InvalidToolInput)
    }

    /// Returns true if the error is due to the service being unavailable.
    pub fn is_unavailable(&self) -> bool {
        matches!(self.error_code, WebFetchErrorCode::Unavailable)
    }

    /// Returns true if the error is due to exceeding the maximum number of uses.
    pub fn is_max_uses_exceeded(&self) -> bool {
        matches!(self.error_code, WebFetchErrorCode::MaxUsesExceeded)
    }

    /// Returns true if the error is due to too many requests.
    pub fn is_too_many_requests(&self) -> bool {
        matches!(self.error_code, WebFetchErrorCode::TooManyRequests)
    }

    /// Returns true if the URL was not in the allowed domains.
    pub fn is_url_not_allowed(&self) -> bool {
        matches!(self.error_code, WebFetchErrorCode::UrlNotAllowed)
    }

    /// Returns true if the fetch operation itself failed.
    pub fn is_fetch_failed(&self) -> bool {
        matches!(self.error_code, WebFetchErrorCode::FetchFailed)
    }

    /// Returns true if the URL was not referenced in the prior context (Anthropic restriction:
    /// only URLs from prior web search results may be fetched).
    pub fn is_url_not_in_prior_context(&self) -> bool {
        matches!(self.error_code, WebFetchErrorCode::UrlNotInPriorContext)
    }

    /// Returns true if the error code was not recognised (forward-compatibility catch-all).
    pub fn is_unknown(&self) -> bool {
        matches!(self.error_code, WebFetchErrorCode::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization() {
        let error = WebFetchToolResultError { error_code: WebFetchErrorCode::InvalidToolInput };
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"error_code":"invalid_tool_input"}"#);
    }

    #[test]
    fn deserialization() {
        let json = r#"{"error_code":"max_uses_exceeded"}"#;
        let error: WebFetchToolResultError = serde_json::from_str(json).unwrap();
        assert_eq!(error.error_code, WebFetchErrorCode::MaxUsesExceeded);
    }

    #[test]
    fn error_code_helpers() {
        let error = WebFetchToolResultError::new(WebFetchErrorCode::InvalidToolInput);
        assert!(error.is_invalid_input());
        assert!(!error.is_unavailable());
        assert!(!error.is_max_uses_exceeded());
        assert!(!error.is_too_many_requests());
        assert!(!error.is_url_not_allowed());
        assert!(!error.is_fetch_failed());
        assert!(!error.is_url_not_in_prior_context());
        assert!(!error.is_unknown());
    }

    #[test]
    fn url_not_in_prior_context_roundtrips() {
        let error = WebFetchToolResultError::new(WebFetchErrorCode::UrlNotInPriorContext);
        assert!(error.is_url_not_in_prior_context());
        // This is the error code the live API returns when a model tries to fetch a URL that
        // wasn't mentioned in prior web_search results.
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"error_code":"url_not_in_prior_context"}"#);
        let deserialized: WebFetchToolResultError = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.error_code, WebFetchErrorCode::UrlNotInPriorContext);
    }

    #[test]
    fn unknown_error_code_deserializes_to_unknown_variant() {
        let json = r#"{"error_code":"some_future_code"}"#;
        let error: WebFetchToolResultError = serde_json::from_str(json).unwrap();
        assert!(error.is_unknown());
    }

    #[test]
    fn url_not_in_prior_context_with_type_field_ignored() {
        // The live API includes a "type" field in the error content that our struct ignores.
        let json =
            r#"{"type":"web_fetch_tool_result_error","error_code":"url_not_in_prior_context"}"#;
        let error: WebFetchToolResultError = serde_json::from_str(json).unwrap();
        assert!(error.is_url_not_in_prior_context());
    }
}
