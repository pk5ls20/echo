use crate::models::dyn_setting::{
    DynSetting, DynSettingCollector, DynSettingsValueBindRow, DynSettingsValueRow,
};
use crate::services::states::db::{DataBaseError, DataBaseResult, SqliteBaseResultExt};
use sqlx::{Executor, Sqlite, SqlitePool, query, query_as};
use std::backtrace::Backtrace;
use std::borrow::Cow;

pub struct DynSettingRepo<'a> {
    pub pool: &'a SqlitePool,
}

impl<'a> DynSettingRepo<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub(in crate::services) async fn get_inner<'c, K, E>(
        &self,
        key: K,
        executor: E,
    ) -> DataBaseResult<DynSettingsValueRow>
    where
        K: Into<Cow<'static, str>>,
        E: Executor<'c, Database = Sqlite>,
    {
        let key_cow = key.into();
        let key_ref = key_cow.as_ref();
        query_as!(
            DynSettingsValueRow,
            "SELECT val FROM system_settings WHERE key = ?",
            key_ref
        )
        .fetch_one(executor)
        .await
        .resolve()
    }

    pub(in crate::services) async fn get_with_dyn_tx<'c, T, E>(
        &self,
        key: T,
        executor: E,
    ) -> DataBaseResult<DynSettingsValueBindRow<T::Value>>
    where
        T: DynSetting,
        E: Executor<'c, Database = Sqlite>,
    {
        let key_str = key.key();
        let db_val = self.get_inner(key_str, executor).await?;
        let val = key
            .parse(&db_val.val)
            .map_err(|e| DataBaseError::DynSettingParse {
                err: e,
                backtrace: Backtrace::capture(),
            })?;
        Ok(DynSettingsValueBindRow { val })
    }

    pub(in crate::services) async fn set_inner<'c, K, E>(
        &self,
        key: K,
        value: &str,
        executor: E,
    ) -> DataBaseResult<()>
    where
        K: Into<Cow<'static, str>>,
        E: Executor<'c, Database = Sqlite>,
    {
        let key_cow = key.into();
        let key_ref = key_cow.as_ref();
        query!(
            r#"
                INSERT INTO system_settings (key, val)
                VALUES (?, ?)
                ON CONFLICT(key) DO
                    UPDATE SET
                    val = excluded.val,
                    updated_at = strftime('%s','now')
            "#,
            key_ref,
            value,
        )
        .execute(executor)
        .await
        .resolve()?;
        Ok(())
    }

    pub async fn initialise(&self) -> DataBaseResult<()> {
        let mut tx = self.pool.begin().await?;
        for (&key, val) in DynSettingCollector::original_kv_map() {
            self.set_inner(key, &val.val, &mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

// TODO: The current implementation is rather hacky and compromises encapsulation.
// TODO: Consider refactoring using "transactions in closure"
#[macro_export]
macro_rules! get_batch_tuple_pure {
    ($store:expr, $($setting:expr),+ $(,)?) => {{
        async {
            let mut tx = $store.pool.begin().await?;
            let res = (
                $(
                    $store.get_with_dyn_tx($setting, &mut *tx).await?.val,
                )+
            );
            tx.commit().await?;
            Ok::<_, $crate::services::states::db::DataBaseError>(res)
        }.await
    }};
}
