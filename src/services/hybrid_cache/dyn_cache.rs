use crate::models::dyn_setting::{
    DynSetting, DynSettingCollector, DynSettingsBindValue, DynSettingsKvMap, DynSettingsValue,
};
use crate::services::hybrid_cache::{HybridCacheError, HybridCacheResult};
use crate::services::states::EchoState;
use crate::services::states::db::DataBaseError;
use crate::utils::smart_to_string::SerdeToString;
use ahash::{HashMap, HashMapExt};
use scc::HashCache;
use sqlx::{Executor, Sqlite};
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

    pub async fn get_inner<'c, K, E>(
        &self,
        key: K,
        executor: E,
    ) -> HybridCacheResult<DynSettingsValue<'static>>
    where
        K: Into<Cow<'static, str>>,
        E: Executor<'c, Database = Sqlite>,
    {
        let key_cow = key.into();
        if let Some(val) = self.cache.get_async(key_cow.as_ref()).await {
            return Ok(val.get().clone());
        }
        let db_row = self
            .state
            .db
            .dyn_settings()
            .get_inner(key_cow.clone(), executor)
            .await?;
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
    ) -> HybridCacheResult<DynSettingsBindValue<'_, T::Value>>
    where
        T: DynSetting,
    {
        let kv = self.get_inner(key.key(), self.state.db.pool()).await?;
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

    pub async fn get_with_dyn_tx<'c, T, E>(
        &self,
        key: T,
        executor: E,
    ) -> HybridCacheResult<DynSettingsBindValue<'_, T::Value>>
    where
        T: DynSetting,
        E: Executor<'c, Database = Sqlite>,
    {
        let key_str = key.key();
        let kv = self.get_inner(key_str, executor).await?;
        let parsed = key.parse(&kv.val).map_err(|e| {
            HybridCacheError::DatabaseError(DataBaseError::DynSettingParse {
                err: e,
                backtrace: Backtrace::capture(),
            })
        })?;
        Ok(DynSettingsBindValue {
            val: parsed,
            description: kv.description,
            side_effects: kv.side_effects,
        })
    }

    pub async fn get_with_str<K>(&self, key: K) -> HybridCacheResult<DynSettingsValue<'static>>
    where
        K: Into<Cow<'static, str>>,
    {
        self.get_inner(key, self.state.db.pool()).await
    }

    pub async fn get_all_kvs(&self) -> HybridCacheResult<DynSettingsKvMap<'static>> {
        let mut tx = self
            .state
            .db
            .pool()
            .begin()
            .await
            .map_err(|e| HybridCacheError::DatabaseError(DataBaseError::SqlxOther(e)))?;
        let ori_map = DynSettingCollector::original_kv_map();
        let mut res = HashMap::with_capacity(ori_map.len());
        for &key in ori_map.keys() {
            let val = self.get_inner(key, &mut *tx).await?;
            res.insert(key, val);
        }
        tx.commit()
            .await
            .map_err(|e| HybridCacheError::DatabaseError(DataBaseError::SqlxOther(e)))?;
        Ok(res)
    }

    pub async fn set_inner<'c, K, E>(
        &self,
        key: K,
        value: &str,
        executor: E,
    ) -> HybridCacheResult<()>
    where
        K: Into<Cow<'static, str>>,
        E: Executor<'c, Database = Sqlite>,
    {
        let key_cow = key.into();
        let key_ref = key_cow.as_ref();
        self.state
            .db
            .dyn_settings()
            .set_inner(key_cow.clone(), value, executor)
            .await?;
        self.cache.remove_async(key_ref).await;
        Ok(())
    }

    pub async fn set_with_dyn<T>(&self, key: T, value: &T::Value) -> HybridCacheResult<()>
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
        self.set_inner(key_str, &val_str, self.state.db.pool())
            .await
    }

    pub async fn set_with_str(&self, key: &str, value: &str) -> HybridCacheResult<()> {
        self.set_inner(key.to_owned(), value, self.state.db.pool())
            .await
    }
}

// TODO: The current implementation is rather hacky and compromises encapsulation.
// TODO: Consider refactoring using "transactions in closure"
#[macro_export]
macro_rules! get_batch_tuple {
    ($dyn_cache:expr, $($setting:expr),+ $(,)?) => {{
        async {
            use $crate::services::hybrid_cache::HybridCacheError;
            use $crate::services::states::db::DataBaseError;
            let mut tx = $dyn_cache
                .state
                .db
                .pool()
                .begin()
                .await
                .map_err(|e| {
                    HybridCacheError::DatabaseError(DataBaseError::SqlxOther(e))
                })?;
            let res = (
                $(
                    $dyn_cache.get_with_dyn_tx($setting, &mut *tx).await?.val,
                )+
            );
            tx.commit()
                .await
                .map_err(|e| {
                    HybridCacheError::DatabaseError(DataBaseError::SqlxOther(e))
                })?;
            Ok::<_, HybridCacheError>(res)
        }
        .await
    }};
}
