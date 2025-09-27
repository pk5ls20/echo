mod dyn_cache;
mod resources;
mod users;

use crate::services::hybrid_cache::dyn_cache::HybridDynCache;
use crate::services::hybrid_cache::resources::HybridResourcesCache;
use crate::services::hybrid_cache::users::HybridUsersCache;
use crate::services::states::EchoState;
use crate::services::states::db::DataBaseError;
use echo_macros::EchoBusinessError;
use std::sync::Arc;

pub struct HybridCacheService {
    state: Arc<EchoState>,
    pub users: HybridUsersCache,
    pub resources: HybridResourcesCache,
    pub dyn_settings: HybridDynCache,
}

#[derive(Debug, thiserror::Error, EchoBusinessError)]
pub enum HybridCacheError {
    #[error("Item not found")]
    ItemNotFound,
    #[error("Partial items not found: found {0} out of {1}")]
    PartialItemNotFound(usize, usize),
    #[error(transparent)]
    DatabaseError(#[from] DataBaseError),
    #[error("Failed to insert item into HashCache")]
    InsertCacheError,
}

pub type HybridCacheResult<T> = Result<T, HybridCacheError>;

impl HybridCacheService {
    pub fn new(state: Arc<EchoState>) -> Self {
        let users = HybridUsersCache::new(&state);
        let resources = HybridResourcesCache::new(&state);
        let dyn_settings = HybridDynCache::new(&state);
        Self {
            state,
            users,
            resources,
            dyn_settings,
        }
    }
}
