use crate::cache::{DbBackedCache, load_from_db};
use crate::entity::token;
use crate::make_global_cache;
use crate::token::get::parse_token_limit_with_compat;
use nodeget_lib::permission::data_structure::Limit;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
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

make_global_cache!(TokenCache, TOKEN_CACHE_GLOBAL);

impl DbBackedCache for TokenCache {
    type Model = token::Model;

    fn cache_name() -> &'static str {
        "token"
    }

    fn build_cache(models: Vec<Self::Model>) -> Self {
        let (by_key, by_username, super_token) = Self::build_maps(models);
        Self {
            inner: RwLock::new(TokenCacheInner {
                by_key,
                by_username,
                super_token,
            }),
        }
    }

    async fn reload_from_models(&self, models: Vec<Self::Model>) {
        let (by_key, by_username, super_token) = Self::build_maps(models);
        let mut guard = self.inner.write().await;
        guard.by_key = by_key;
        guard.by_username = by_username;
        guard.super_token = super_token;
        drop(guard);
    }

    fn load_all() -> impl Future<Output = anyhow::Result<Vec<Self::Model>>> + Send {
        load_from_db::<token::Entity>()
    }
}

impl TokenCache {
    fn build_maps(
        all_tokens: Vec<token::Model>,
    ) -> (
        HashMap<String, Arc<CachedToken>>,
        HashMap<String, Arc<CachedToken>>,
        Option<Arc<CachedToken>>,
    ) {
        let mut by_key = HashMap::with_capacity(all_tokens.len());
        let mut by_username = HashMap::new();
        let mut super_token: Option<Arc<CachedToken>> = None;

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
}
