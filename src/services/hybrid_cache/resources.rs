use crate::models::DiffRef;
use crate::models::resource::{ResourceItemWithRefRaw, ResourceReferenceInner};
use crate::services::hybrid_cache::{HybridCacheError, HybridCacheResult};
use crate::services::states::EchoState;
use crate::services::states::db::{DataBaseError, EchoDatabaseExecutor};
use scc::HashCache;
use smallvec::SmallVec;
use std::sync::Arc;

pub struct HybridResourcesCache {
    state: Arc<EchoState>,
    cache: HashCache<i64, Arc<ResourceItemWithRefRaw>>,
}

impl HybridResourcesCache {
    pub fn new(state: &Arc<EchoState>) -> Self {
        let cache = HashCache::with_capacity(0, state.config.perf.res_cache_capacity);
        Self {
            cache,
            state: state.clone(),
        }
    }

    pub async fn update_resource(
        &self,
        ref_diff: DiffRef<'_, ResourceReferenceInner>,
    ) -> HybridCacheResult<()> {
        let res_ids: SmallVec<[_; 8]> = ref_diff.iter().map(|r| r.value.res_id).collect();
        self.state
            .db
            .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
                exec.resources().update_resource(ref_diff).await
            })
            .await?;
        for res_id in res_ids {
            self.cache.remove_async(&res_id).await;
        }
        Ok(())
    }

    pub async fn delete_resources_batch(
        &self,
        res: &[ResourceReferenceInner],
    ) -> HybridCacheResult<()> {
        let res_ids: SmallVec<[_; 8]> = res.iter().map(|r| r.res_id).collect();
        self.state
            .db
            .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
                exec.resources().delete_resources_batch(res).await
            })
            .await?;
        for res_id in res_ids {
            self.cache.remove_async(&res_id).await;
        }
        Ok(())
    }

    pub async fn get_resources_by_id(
        &self,
        res_ids: &[i64],
    ) -> HybridCacheResult<Vec<Arc<ResourceItemWithRefRaw>>> {
        let mut results = vec![None; res_ids.len()];
        let mut missing_res_id: SmallVec<[_; 8]> = SmallVec::new();
        let mut missing_res_idx: SmallVec<[_; 8]> = SmallVec::new();
        for (idx, &id) in res_ids.iter().enumerate() {
            if let Some(cached) = self.cache.get_async(&id).await {
                results[idx] = Some(cached.get().clone());
            } else {
                missing_res_id.push(id);
                missing_res_idx.push(idx);
            }
        }
        // TODO: RustRover cannot infer the type here, so fxxk u jetbrains!
        let db_res: Vec<(Option<Arc<ResourceItemWithRefRaw>>, usize)> = self
            .state
            .db
            .single(async |mut exec: EchoDatabaseExecutor<'_>| {
                let res_rows_arc = exec
                    .resources()
                    .get_resource_by_id_batch(&missing_res_id[..])
                    .await?
                    .into_iter()
                    .map(|opt| opt.map(Arc::new))
                    .zip(missing_res_idx)
                    .collect::<Vec<_>>();
                Ok::<_, DataBaseError>(res_rows_arc)
            })
            .await
            .map_err(HybridCacheError::DatabaseError)?;
        if db_res.len() != missing_res_id.len() {
            return Err(HybridCacheError::PartialItemNotFound(
                db_res.len(),
                missing_res_id.len(),
            ));
        }
        for (res, idx) in db_res {
            match res {
                Some(res) => {
                    self.cache
                        .put_async(res.id, res.clone())
                        .await
                        .map_err(|_| HybridCacheError::InsertCacheError)?;
                    results[idx] = Some(res);
                }
                None => return Err(HybridCacheError::ItemNotFound),
            }
        }
        let db_res = results
            .into_iter()
            .map(|opt| opt.unwrap()) // safety
            .collect();
        Ok(db_res)
    }
}
