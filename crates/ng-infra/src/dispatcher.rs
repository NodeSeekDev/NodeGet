//! RPC dispatcher trait.
//!
//! [`RpcDispatcher`] abstracts over the RPC framework to allow
//! module merging without coupling to a specific implementation.

/// Trait for RPC method dispatch.
///
/// Concrete implementations (e.g. wrapping jsonrpsee's `RpcModule`)
/// provide framework-agnostic module assembly.
pub trait RpcDispatcher: Send + Sync + Sized {
    /// Merge another dispatcher into this one.
    ///
    /// After merging, all methods from `other` become available
    /// through `self`.
    fn merge(&mut self, other: Self) -> anyhow::Result<()>;
}
