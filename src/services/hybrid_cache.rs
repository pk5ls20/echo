mod dyn_cache;
mod users;

use crate::services::hybrid_cache::dyn_cache::HybridDynCache;
use crate::services::hybrid_cache::users::HybridUsersCache;
use crate::services::states::EchoState;
use crate::services::states::db::DataBaseError;
use std::sync::Arc;

pub struct HybridCacheService {
    state: Arc<EchoState>,
    pub users: HybridUsersCache,
    pub dyn_settings: HybridDynCache,
}

#[derive(Debug, thiserror::Error)]
pub enum HybridCacheError {
    #[error("Item not found")]
    ItemNotFound,
    #[error(transparent)]
    DatabaseError(#[from] DataBaseError),
    #[error("Failed to insert item into HashCache")]
    InsertCacheError,
}

pub type HybridCacheResult<T> = Result<T, HybridCacheError>;

impl HybridCacheService {
    pub fn new(state: Arc<EchoState>) -> Self {
        let users = HybridUsersCache::new(&state);
        let dyn_settings = HybridDynCache::new(&state);
        Self {
            state,
            users,
            dyn_settings,
        }
    }
}
