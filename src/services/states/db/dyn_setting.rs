use crate::models::dyn_setting::{
    DynSetting, DynSettingCollector, DynSettingsValueBindRow, DynSettingsValueRow,
};
use crate::services::states::db::{DataBaseError, DataBaseResult, SqliteBaseResultExt};
use sqlx::{Executor, Sqlite, query, query_as};
use std::backtrace::Backtrace;
use std::borrow::Cow;

pub struct DynSettingsRepo<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    pub inner: &'a mut E,
}

impl<'a, E> DynSettingsRepo<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    pub(in crate::services) async fn get_inner<K>(
        &mut self,
        key: K,
    ) -> DataBaseResult<DynSettingsValueRow>
    where
        K: Into<Cow<'static, str>>,
    {
        let key_cow = key.into();
        let key_ref = key_cow.as_ref();
        query_as!(
            DynSettingsValueRow,
            "SELECT val FROM system_settings WHERE key = ?",
            key_ref
        )
        .fetch_one(&mut *self.inner)
        .await
        .resolve()
    }

    pub(in crate::services) async fn get_with_dyn_tx<K>(
        &mut self,
        key: K,
    ) -> DataBaseResult<DynSettingsValueBindRow<K::Value>>
    where
        K: DynSetting,
    {
        let key_str = key.key();
        let db_val = self.get_inner(key_str).await?;
        let val = key
            .parse(&db_val.val)
            .map_err(|e| DataBaseError::DynSettingParse {
                err: e,
                backtrace: Backtrace::capture(),
            })?;
        Ok(DynSettingsValueBindRow { val })
    }

    pub(in crate::services) async fn set_inner<K>(
        &mut self,
        key: K,
        value: &str,
        overwrite: bool,
    ) -> DataBaseResult<()>
    where
        K: Into<Cow<'static, str>>,
    {
        let key_cow = key.into();
        let key_ref = key_cow.as_ref();
        match overwrite {
            true => {
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
                .execute(&mut *self.inner)
                .await
                .resolve()?;
            }
            false => {
                query!(
                    "INSERT OR IGNORE INTO system_settings (key, val) VALUES (?, ?)",
                    key_ref,
                    value,
                )
                .execute(&mut *self.inner)
                .await
                .resolve()?;
            }
        }
        Ok(())
    }

    pub async fn initialise(&mut self) -> DataBaseResult<()> {
        for (&key, val) in DynSettingCollector::original_kv_map() {
            self.set_inner(key, &val.val, false).await?;
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! get_batch_tuple_pure {
    ($state:expr, $($setting:expr),+ $(,)?) => {{
        async {
            use $crate::services::states::db::{DataBaseError, EchoDatabaseExecutor};
            let res = $state
                .transaction(async |mut exec: EchoDatabaseExecutor<'_>|{
                    let out = (
                        $(
                            {
                                let bind = exec
                                    .dyn_settings()
                                    .get_with_dyn_tx($setting)
                                    .await?;
                                bind.val
                            },
                        )+
                    );
                    Ok::<_, DataBaseError>(out)
                })
                .await?;
            Ok::<_, DataBaseError>(res)
        }.await
    }};
}
