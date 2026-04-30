use crate::DB;
use crate::entity::token;
use crate::token::get::parse_token_limit_with_compat;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::Limit;
use sea_orm::EntityTrait;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

/// Pre-parsed token entry: model + parsed `token_limit`.
/// Avoids re-parsing `serde_json::Value` on every auth call.
pub struct CachedToken {
    pub model: Arc<token::Model>,
    pub parsed_limits: Vec<Limit>,
}

struct TokenCacheInner {
    /// `token_key` -> cached entry
    by_key: HashMap<String, Arc<CachedToken>>,
    /// username -> cached entry (only tokens that have a username)
    by_username: HashMap<String, Arc<CachedToken>>,
    /// super token (id=1), cached separately for fast access
    super_token: Option<Arc<CachedToken>>,
}

pub struct TokenCache {
    inner: RwLock<TokenCacheInner>,
}

static TOKEN_CACHE: OnceLock<TokenCache> = OnceLock::new();

impl TokenCache {
    /// Initialize the global token cache by loading all tokens from DB.
    /// Must be called after DB is initialized and super token is created.
    pub async fn init() -> anyhow::Result<()> {
        let db = DB.get().ok_or_else(|| {
            NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
        })?;

        let all_tokens = token::Entity::find()
            .all(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Failed to load tokens: {e}")))?;

        let (by_key, by_username, super_token) = Self::build_cache(all_tokens);

        let cache = Self {
            inner: RwLock::new(TokenCacheInner {
                by_key,
                by_username,
                super_token,
            }),
        };

        if TOKEN_CACHE.set(cache).is_err() {
            // Already initialized — just reload instead
            tracing::warn!(target: "token", "Token cache already initialized, reloading");
            Self::reload().await?;
        } else {
            tracing::info!(target: "token", "Token cache initialized");
        }

        Ok(())
    }

    /// Check if the global token cache has been initialized.
    pub fn is_initialized() -> bool {
        TOKEN_CACHE.get().is_some()
    }

    /// Get the global token cache instance.
    pub fn global() -> &'static Self {
        TOKEN_CACHE
            .get()
            .expect("Token cache not initialized — call TokenCache::init() first")
    }

    /// Reload all tokens from DB into cache.
    /// Called after any CUD operation on the token table.
    /// No-op if cache hasn't been initialized yet (e.g. during startup).
    pub async fn reload() -> anyhow::Result<()> {
        let Some(cache) = TOKEN_CACHE.get() else {
            // Cache not yet initialized — init() will load everything
            return Ok(());
        };
        let db = DB.get().ok_or_else(|| {
            NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
        })?;

        let all_tokens = token::Entity::find()
            .all(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Failed to reload tokens: {e}")))?;

        let (by_key, by_username, super_token) = Self::build_cache(all_tokens);

        let mut guard = cache.inner.write().await;
        guard.by_key = by_key;
        guard.by_username = by_username;
        guard.super_token = super_token;
        drop(guard);

        tracing::debug!(target: "token", "Token cache reloaded");
        Ok(())
    }

    /// Find a cached token by `token_key`.
    pub async fn find_by_key(&self, key: &str) -> Option<Arc<CachedToken>> {
        let guard = self.inner.read().await;
        guard.by_key.get(key).map(Arc::clone)
    }

    /// Find a cached token by username.
    pub async fn find_by_username(&self, username: &str) -> Option<Arc<CachedToken>> {
        let guard = self.inner.read().await;
        guard.by_username.get(username).map(Arc::clone)
    }

    /// Get the super token (id=1).
    pub async fn get_super_token(&self) -> Option<Arc<CachedToken>> {
        let guard = self.inner.read().await;
        guard.super_token.as_ref().map(Arc::clone)
    }

    /// Get all cached tokens (for `list_all_tokens`).
    pub async fn get_all(&self) -> Vec<Arc<CachedToken>> {
        let guard = self.inner.read().await;
        guard.by_key.values().map(Arc::clone).collect()
    }

    /// Build cache maps from a list of models.
    fn build_cache(
        all_tokens: Vec<token::Model>,
    ) -> (
        HashMap<String, Arc<CachedToken>>,
        HashMap<String, Arc<CachedToken>>,
        Option<Arc<CachedToken>>,
    ) {
        let mut by_key = HashMap::with_capacity(all_tokens.len());
        let mut by_username = HashMap::new();
        let mut super_token = None;

        for model in all_tokens {
            let parsed_limits = parse_token_limit_with_compat(model.token_limit.clone())
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        target: "token",
                        token_key = %model.token_key,
                        error = %e,
                        "failed to pre-parse token_limit, using empty"
                    );
                    Vec::new()
                });

            let cached = Arc::new(CachedToken {
                model: Arc::new(model),
                parsed_limits,
            });

            if cached.model.id == 1 {
                super_token = Some(Arc::clone(&cached));
            }
            by_key.insert(cached.model.token_key.clone(), Arc::clone(&cached));
            if let Some(ref uname) = cached.model.username {
                by_username.insert(uname.clone(), cached);
            }
        }

        (by_key, by_username, super_token)
    }
}
