//! Permission handling for ACP agent tool calls.
//!
//! ACP agents request permission before sensitive operations (file deletion,
//! package installs, network requests). This module provides the callback
//! mechanism for ADK agents to approve or deny these requests.

use std::fmt;

/// A permission request from an ACP agent.
///
/// The agent wants to perform a sensitive operation and is asking for approval.
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    /// Human-readable description of what the agent wants to do.
    pub title: String,
    /// Available options (e.g., "allow_once", "allow_always", "deny").
    pub options: Vec<PermissionOption>,
}

/// A single permission option.
#[derive(Debug, Clone)]
pub struct PermissionOption {
    /// Machine-readable option ID (e.g., "allow_once").
    pub id: String,
    /// Human-readable name (e.g., "Yes, allow this once").
    pub name: String,
}

/// The decision made by the permission handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Allow this specific operation (select the given option ID).
    Allow(String),
    /// Deny the operation.
    Deny,
}

impl PermissionDecision {
    /// Allow once (selects the first available option).
    pub fn allow_once() -> Self {
        Self::Allow("allow_once".to_string())
    }

    /// Deny the operation.
    pub fn deny() -> Self {
        Self::Deny
    }
}

impl fmt::Display for PermissionDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow(id) => write!(f, "allow({id})"),
            Self::Deny => write!(f, "deny"),
        }
    }
}

/// Policy for handling permission requests from ACP agents.
///
/// # Example
///
/// ```rust,ignore
/// use adk_acp::permissions::{PermissionPolicy, PermissionRequest, PermissionDecision};
///
/// // Auto-approve everything (YOLO mode — for development only)
/// let policy = PermissionPolicy::AutoApprove;
///
/// // Deny everything (safe mode)
/// let policy = PermissionPolicy::DenyAll;
///
/// // Custom logic
/// let policy = PermissionPolicy::Custom(Box::new(|req| {
///     if req.title.contains("delete") || req.title.contains("rm ") {
///         PermissionDecision::deny()
///     } else {
///         PermissionDecision::allow_once()
///     }
/// }));
/// ```
#[derive(Default)]
pub enum PermissionPolicy {
    /// Auto-approve all permission requests. Use only in development/trusted environments.
    #[default]
    AutoApprove,
    /// Deny all permission requests. Safe but limits agent capabilities.
    DenyAll,
    /// Custom decision function.
    Custom(Box<dyn Fn(&PermissionRequest) -> PermissionDecision + Send + Sync>),
}

impl PermissionPolicy {
    /// Evaluate the policy for a given request.
    pub fn decide(&self, request: &PermissionRequest) -> PermissionDecision {
        match self {
            Self::AutoApprove => {
                // Select the first option, or use "allow_once" as fallback
                request
                    .options
                    .first()
                    .map(|opt| PermissionDecision::Allow(opt.id.clone()))
                    .unwrap_or_else(PermissionDecision::deny)
            }
            Self::DenyAll => PermissionDecision::Deny,
            Self::Custom(f) => f(request),
        }
    }
}

impl fmt::Debug for PermissionPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AutoApprove => write!(f, "PermissionPolicy::AutoApprove"),
            Self::DenyAll => write!(f, "PermissionPolicy::DenyAll"),
            Self::Custom(_) => write!(f, "PermissionPolicy::Custom(...)"),
        }
    }
}
