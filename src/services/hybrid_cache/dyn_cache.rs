use crate::models::dyn_setting::{
    DynSetting, DynSettingCollector, DynSettingsBindValue, DynSettingsKvMap, DynSettingsValue,
};
use crate::services::hybrid_cache::{HybridCacheError, HybridCacheResult};
use crate::services::states::EchoState;
use crate::services::states::db::{DataBaseError, EchoDatabaseExecutor};
use crate::utils::smart_to_string::SerdeToString;
use ahash::{HashMap, HashMapExt};
use scc::HashCache;
use std::backtrace::Backtrace;
use std::borrow::Cow;
use std::sync::Arc;

pub struct HybridDynCache {
    pub state: Arc<EchoState>,
    cache: HashCache<Cow<'static, str>, DynSettingsValue<'static>>,
}

impl HybridDynCache {
    pub fn new(state: &Arc<EchoState>) -> Self {
        Self {
            state: state.clone(),
            cache: HashCache::with_capacity(0, state.config.perf.dyn_setting_cache_capacity),
        }
    }

    pub async fn get_inner<K>(
        &self,
        key: K,
        exec: &mut EchoDatabaseExecutor<'_>,
    ) -> HybridCacheResult<DynSettingsValue<'static>>
    where
        K: Into<Cow<'static, str>>,
    {
        let key_cow = key.into();
        if let Some(val) = self.cache.get_async(key_cow.as_ref()).await {
            return Ok(val.get().clone());
        }
        let db_row = exec.dyn_settings().get_inner(key_cow.clone()).await?;
        let ori_const_val = DynSettingCollector::original_kv_map()
            .get(key_cow.as_ref())
            .ok_or(HybridCacheError::ItemNotFound)?;
        let out = DynSettingsValue {
            val: db_row.val,
            description: ori_const_val.description.clone(),
            side_effects: ori_const_val.side_effects.clone(),
        };
        self.cache
            .put_async(key_cow, out.clone())
            .await
            .map_err(|_| HybridCacheError::InsertCacheError)?;
        Ok(out)
    }

    pub async fn get_with_dyn<T>(
        &self,
        key: T,
        exec: &mut EchoDatabaseExecutor<'_>,
    ) -> HybridCacheResult<DynSettingsBindValue<'_, T::Value>>
    where
        T: DynSetting + Send + Sync,
    {
        let kv = self.get_inner(key.key(), exec).await?;
        let parsed = key
            .parse(&kv.val)
            .map_err(|e| DataBaseError::DynSettingParse {
                err: e,
                backtrace: Backtrace::capture(),
            })?;
        Ok(DynSettingsBindValue {
            val: parsed,
            description: kv.description,
            side_effects: kv.side_effects,
        })
    }

    pub async fn get_with_str<K>(
        &self,
        key: K,
        exec: &mut EchoDatabaseExecutor<'_>,
    ) -> HybridCacheResult<DynSettingsValue<'static>>
    where
        K: Into<Cow<'static, str>>,
    {
        self.get_inner(key, exec).await
    }

    pub async fn get_all_kvs(&self) -> HybridCacheResult<DynSettingsKvMap<'static>> {
        let ori_map = DynSettingCollector::original_kv_map();
        let res = self
            .state
            .db
            .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
                let mut res = HashMap::with_capacity(ori_map.len());
                for &key in ori_map.keys() {
                    let val = self.get_inner(key, &mut exec).await?;
                    res.insert(key, val);
                }
                Ok::<_, HybridCacheError>(res)
            })
            .await?;
        Ok(res)
    }

    pub async fn set_inner<K>(&self, key: K, value: &str, overwrite: bool) -> HybridCacheResult<()>
    where
        K: Into<Cow<'static, str>>,
    {
        let key_cow = key.into();
        let key_ref = key_cow.as_ref();
        self.state
            .db
            .single(async |mut exec: EchoDatabaseExecutor<'_>| {
                exec.dyn_settings()
                    .set_inner(key_cow.clone(), value, overwrite)
                    .await
            })
            .await?;
        self.cache.remove_async(key_ref).await;
        Ok(())
    }

    pub async fn set_with_dyn<T>(
        &self,
        key: T,
        value: &T::Value,
        overwrite: bool,
    ) -> HybridCacheResult<()>
    where
        T: DynSetting,
    {
        let key_str = key.key();
        let val_str = value.smart_to_string().map_err(|e| {
            HybridCacheError::DatabaseError(DataBaseError::DynSettingParse {
                err: e,
                backtrace: Backtrace::capture(),
            })
        })?;
        self.set_inner(key_str, &val_str, overwrite).await
    }

    pub async fn set_with_str(
        &self,
        key: &str,
        value: &str,
        overwrite: bool,
    ) -> HybridCacheResult<()> {
        self.set_inner(key.to_owned(), value, overwrite).await
    }
}

#[macro_export]
macro_rules! get_batch_tuple {
    ($dyn_cache:expr, $($setting:expr),+ $(,)?) => {{
        async {
            use $crate::services::hybrid_cache::HybridCacheError;
            let res = $dyn_cache
                .state
                .db
                .transaction(async |mut exec| {
                    let out = (
                        $(
                            {
                                let bind = $dyn_cache
                                    .get_with_dyn($setting, &mut exec)
                                    .await?;
                                bind.val
                            },
                        )+
                    );
                    Ok::<_, HybridCacheError>(out)
                })
                .await?;
            Ok::<_, HybridCacheError>(res)
        }.await
    }};
}
