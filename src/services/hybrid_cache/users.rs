use crate::models::users::{User, UserInternal, UserRowOptional};
use crate::services::hybrid_cache::{HybridCacheError, HybridCacheResult};
use crate::services::states::EchoState;
use crate::services::states::db::EchoDatabaseExecutor;
use scc::HashCache;
use std::sync::Arc;
use time::OffsetDateTime;

pub struct HybridUsersCache {
    state: Arc<EchoState>,
    cache: HashCache<i64, Arc<User>>,
}

impl HybridUsersCache {
    pub fn new(state: &Arc<EchoState>) -> Self {
        let cache = HashCache::with_capacity(0, state.config.perf.user_cache_capacity);
        Self {
            cache,
            state: state.clone(),
        }
    }

    pub async fn get_user_by_user_id(&self, user_id: i64) -> HybridCacheResult<Arc<User>> {
        if let Some(user) = self.cache.get_async(&user_id).await {
            return Ok(user.get().clone());
        }
        let db_res = self
            .state
            .db
            .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
                let user_row = exec
                    .users()
                    .query_user_by_id(user_id)
                    .await?
                    .ok_or(HybridCacheError::ItemNotFound)?;
                let user_permission = exec
                    .permission()
                    .combined_query_user_permission(user_row.id, &user_row.role)
                    .await?;
                let res = UserInternal {
                    inner: user_row,
                    permissions: user_permission,
                }
                .into_public();
                Ok::<_, HybridCacheError>(res)
            })
            .await?;
        let db_res = Arc::new(db_res);
        self.cache
            .put_async(user_id, db_res.clone())
            .await
            .map_err(|_| HybridCacheError::InsertCacheError)?;
        Ok(db_res)
    }

    pub async fn update_user(&self, updated_user: UserRowOptional) -> HybridCacheResult<()> {
        let user_id = updated_user.id;
        self.state
            .db
            .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
                exec.users().update_user(updated_user).await
            })
            .await?;
        self.cache.remove_async(&user_id).await;
        Ok(())
    }

    pub async fn remove_user_by_id(&self, user_id: i64) -> HybridCacheResult<()> {
        self.state
            .db
            .single(async |mut exec: EchoDatabaseExecutor<'_>| {
                exec.users().remove_user_by_id(user_id).await
            })
            .await?;
        self.cache.remove_async(&user_id).await;
        Ok(())
    }

    pub async fn grant_user_permission(
        &self,
        user_id: i64,
        assigner_id: i64,
        permission_ids: &[i64],
        exp_time: Option<OffsetDateTime>,
    ) -> HybridCacheResult<()> {
        self.state
            .db
            .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
                exec.permission()
                    .grant_user_permission(user_id, assigner_id, permission_ids, exp_time)
                    .await
            })
            .await?;
        self.cache.remove_async(&user_id).await;
        Ok(())
    }

    pub async fn revoke_user_permission(
        &self,
        user_id: i64,
        permission_ids: &[i64],
    ) -> HybridCacheResult<()> {
        self.state
            .db
            .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
                exec.permission()
                    .revoke_permissions(user_id, permission_ids)
                    .await
            })
            .await?;
        self.cache.remove_async(&user_id).await;
        Ok(())
    }
}
