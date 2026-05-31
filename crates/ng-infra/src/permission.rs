//! Permission types and resolvers.
//!
//! [`ScopedPermission<T>`] is a generic scope-restriction enum used to
//! express "all access" vs "restricted to specific items" for any domain type.
//!
//! [`PermissionResolver`] is the trait that concrete implementations
//! (e.g. the server's token-based permission checker) must satisfy.

use ng_core::permission::data_structure::{Permission, Scope, Token};
use serde::{Deserialize, Serialize};

// ── ScopedPermission ──────────────────────────────────────────────────

/// Permission scope restriction enum.
///
/// `All` — no restriction, full access to all scopes.
/// `Scoped(Vec<T>)` — access restricted to the listed items.
///
/// Uses `Vec<T>` instead of `HashSet<T>` so that the `Eq` bound suffices
/// (the concrete `Scope` type from ng-core does not implement `Hash`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopedPermission<T> {
    /// Full access — no scope restrictions.
    #[default]
    All,
    /// Restricted access — only the listed items are permitted.
    Scoped(Vec<T>),
}

impl<T: Eq> ScopedPermission<T> {
    /// Check if a specific item is permitted.
    pub fn is_allowed(&self, item: &T) -> bool {
        match self {
            Self::All => true,
            Self::Scoped(items) => items.contains(item),
        }
    }

    /// Returns `true` if there are no restrictions.
    pub const fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// Returns the inner list if scoped, `None` if `All`.
    pub fn as_scoped(&self) -> Option<&[T]> {
        match self {
            Self::All => None,
            Self::Scoped(items) => Some(items),
        }
    }
}

// ── PermissionResolver ────────────────────────────────────────────────

/// Trait for resolving permissions for a token.
///
/// Implementations determine the effective scope restrictions
/// for a given token and permission combination.
pub trait PermissionResolver: Send + Sync {
    /// Resolve the effective scope restriction for a permission.
    ///
    /// Returns [`ScopedPermission::All`] if the token has unrestricted access,
    /// or [`ScopedPermission::Scoped`] with the allowed scopes.
    fn resolve(&self, token: &Token, permission: &Permission) -> ScopedPermission<Scope>;
}
