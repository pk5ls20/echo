use crate::models::mfa::{
    MFAAuthMethod, MFAOpType, MfaAuthLog, MfaInfo, MfaSettings, NewMfaAuthLog, NewMfaAuthLogInfo,
    NewTotpCredential, NewWebauthnCredential, TotpCredential, WebauthnCredential,
};
use crate::services::states::db::{
    DataBaseResult, PageQueryBinder, PageQueryResult, SqliteBaseResultExt,
};
use sqlx::{Executor, Sqlite, SqlitePool, query, query_as, query_scalar};
use time::OffsetDateTime;
use uuid::Uuid;

pub struct MfaRepo<'a> {
    pool: &'a SqlitePool,
}

impl<'a> MfaRepo<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    async fn insert_mfa_op_log_with_ctx<'c, E>(
        &self,
        executor: E,
        log: NewMfaAuthLog,
    ) -> DataBaseResult<i64>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        query!(
            r#"
                INSERT INTO mfa_op_logs
                (user_id, op_type, auth_method, is_success, ip_address, user_agent, credential_id, error_message)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            log.user_id,
            log.op_type,
            log.info.auth_method,
            log.info.is_success,
            log.info.ip_address,
            log.info.user_agent,
            log.info.credential_id,
            log.info.error_message
        )
        .execute(executor)
        .await
        .resolve()
        .map(|result| result.last_insert_rowid())
    }

    async fn get_mfa_info<'c, E>(
        &self,
        user_id: i64,
        executor: E,
    ) -> DataBaseResult<Option<MfaSettings>>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        query_as!(
            MfaSettings,
            r#"
                SELECT
                    user_id,
                    mfa_enabled AS "mfa_enabled: bool",
                    updated_at AS "updated_at: OffsetDateTime"
                FROM mfa_infos
                WHERE user_id = ?
            "#,
            user_id
        )
        .fetch_optional(executor)
        .await
        .resolve()
    }

    pub async fn is_mfa_enabled(&self, user_id: i64) -> DataBaseResult<bool> {
        query_scalar!(
            // language=sql
            r#"
                SELECT mfa_enabled AS "mfa_enabled: bool"
                FROM mfa_infos
                WHERE user_id = ?
            "#,
            user_id
        )
        .fetch_optional(self.pool)
        .await
        .resolve()
        .map(|opt| opt.unwrap_or(false))
    }

    pub async fn enable_mfa(&self, user_id: i64) -> DataBaseResult<()> {
        query!(
            // language=sql
            r#"
                INSERT INTO mfa_infos (user_id, mfa_enabled, updated_at)
                VALUES (?, 1, strftime('%s','now'))
                ON CONFLICT(user_id) DO UPDATE SET mfa_enabled = 1, updated_at = excluded.updated_at
            "#,
            user_id
        )
        .execute(self.pool)
        .await
        .resolve()?;
        Ok(())
    }

    pub async fn insert_mfa_op_access_log(
        &self,
        user_id: i64,
        info: NewMfaAuthLog,
    ) -> DataBaseResult<i64> {
        self.insert_mfa_op_log_with_ctx(
            self.pool,
            NewMfaAuthLog {
                user_id,
                op_type: info.op_type,
                info: info.info,
            },
        )
        .await
    }

    pub async fn get_mfa_op_logs_page(
        &self,
        user_id: i64,
        page: PageQueryBinder,
    ) -> DataBaseResult<PageQueryResult<MfaAuthLog>> {
        page.query_page_ctx(|pq| async move {
            query_as!(
                MfaAuthLog,
                // language=sql
                r#"
                    SELECT
                        id,
                        user_id,
                        op_type AS "op_type: MFAOpType",
                        auth_method AS "auth_method: MFAAuthMethod",
                        is_success AS "is_success: bool",
                        ip_address,
                        user_agent,
                        credential_id,
                        error_message,
                        time AS "time: OffsetDateTime"
                    FROM mfa_op_logs
                    WHERE user_id = ?
                    AND id > ?
                    ORDER BY id
                    LIMIT ?
                "#,
                user_id,
                pq.start_after,
                pq.limit,
            )
            .fetch_all(self.pool)
            .await
        })
        .await
    }

    pub async fn insert_totp_credential(
        &self,
        credential: NewTotpCredential,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> DataBaseResult<i64> {
        let mut tx = self.pool.begin().await.resolve()?;
        let credential_id = query!(
            "INSERT INTO totp_credentials (user_id, totp_credential_data) VALUES (?, ?)",
            credential.user_id,
            credential.totp_credential_data
        )
        .execute(&mut *tx)
        .await
        .resolve()?
        .last_insert_rowid();
        self.insert_mfa_op_log_with_ctx(
            &mut *tx,
            NewMfaAuthLog {
                user_id: credential.user_id,
                op_type: MFAOpType::Add,
                info: NewMfaAuthLogInfo {
                    auth_method: MFAAuthMethod::Totp,
                    is_success: true,
                    ip_address,
                    user_agent,
                    credential_id: Some(credential_id),
                    error_message: None,
                },
            },
        )
        .await?;
        tx.commit().await.resolve()?;
        Ok(credential_id)
    }

    pub async fn delete_totp_credential(
        &self,
        user_id: i64,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> DataBaseResult<()> {
        let mut tx = self.pool.begin().await.resolve()?;
        let credential_id = query_scalar!(
            // language=sql
            "SELECT id FROM totp_credentials WHERE user_id = ?",
            user_id
        )
        .fetch_optional(&mut *tx)
        .await
        .resolve()?;
        query!("DELETE FROM totp_credentials WHERE user_id = ?", user_id)
            .execute(&mut *tx)
            .await
            .resolve()?;
        self.insert_mfa_op_log_with_ctx(
            &mut *tx,
            NewMfaAuthLog {
                user_id,
                op_type: MFAOpType::Delete,
                info: NewMfaAuthLogInfo {
                    auth_method: MFAAuthMethod::Totp,
                    is_success: true,
                    ip_address,
                    user_agent,
                    credential_id,
                    error_message: None,
                },
            },
        )
        .await?;
        tx.commit().await.resolve()?;
        Ok(())
    }

    pub async fn list_user_totp_credential(
        &self,
        user_id: i64,
    ) -> DataBaseResult<Option<TotpCredential>> {
        query_as!(
            TotpCredential,
            r#"
                SELECT
                    id,
                    user_id,
                    totp_credential_data,
                    created_at AS "created_at: OffsetDateTime",
                    updated_at AS "updated_at: OffsetDateTime",
                    last_used_at AS "last_used_at?: OffsetDateTime"
                FROM totp_credentials
                WHERE user_id = ?
            "#,
            user_id
        )
        .fetch_optional(self.pool)
        .await
        .resolve()
    }

    pub async fn update_totp_last_used(&self, user_id: i64) -> DataBaseResult<()> {
        query!(
            r#"
                UPDATE totp_credentials
                SET last_used_at = strftime('%s', 'now'),
                    updated_at = strftime('%s', 'now')
                WHERE user_id = ?
            "#,
            user_id
        )
        .execute(self.pool)
        .await
        .resolve()?;
        Ok(())
    }

    pub async fn insert_webauthn_credential(
        &self,
        credential: NewWebauthnCredential,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> DataBaseResult<i64> {
        let mut tx = self.pool.begin().await.resolve()?;
        let credential_id = query!(
            r#"
                INSERT INTO webauthn_credentials
                (user_id, user_unique_uuid, user_name, user_display_name, credential_data)
                VALUES (?, ?, ?, ?, ?)
            "#,
            credential.user_id,
            credential.user_unique_uuid,
            credential.user_name,
            credential.user_display_name,
            credential.credential_data
        )
        .execute(&mut *tx)
        .await
        .resolve()?
        .last_insert_rowid();
        self.insert_mfa_op_log_with_ctx(
            &mut *tx,
            NewMfaAuthLog {
                user_id: credential.user_id,
                op_type: MFAOpType::Add,
                info: NewMfaAuthLogInfo {
                    auth_method: MFAAuthMethod::Webauthn,
                    is_success: true,
                    ip_address,
                    user_agent,
                    credential_id: Some(credential_id),
                    error_message: None,
                },
            },
        )
        .await?;
        tx.commit().await.resolve()?;
        Ok(credential_id)
    }

    pub async fn delete_webauthn_credential(
        &self,
        credential_id: i64,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> DataBaseResult<()> {
        let mut tx = self.pool.begin().await.resolve()?;
        let user_id = query_scalar!(
            // language=sql
            "SELECT user_id FROM webauthn_credentials WHERE id = ?",
            credential_id
        )
        .fetch_optional(&mut *tx)
        .await
        .resolve()?;
        if let Some(user_id) = user_id {
            query!(
                "DELETE FROM webauthn_credentials WHERE id = ?",
                credential_id
            )
            .execute(&mut *tx)
            .await
            .resolve()?;
            self.insert_mfa_op_log_with_ctx(
                &mut *tx,
                NewMfaAuthLog {
                    user_id,
                    op_type: MFAOpType::Delete,
                    info: NewMfaAuthLogInfo {
                        auth_method: MFAAuthMethod::Webauthn,
                        is_success: true,
                        ip_address,
                        user_agent,
                        credential_id: Some(credential_id),
                        error_message: None,
                    },
                },
            )
            .await?;
        }
        tx.commit().await.resolve()?;
        Ok(())
    }

    pub async fn list_user_webauthn_credentials(
        &self,
        user_id: i64,
    ) -> DataBaseResult<Vec<WebauthnCredential>> {
        query_as!(
            WebauthnCredential,
            r#"
                SELECT
                    id,
                    user_id,
                    user_unique_uuid AS "user_unique_uuid: Uuid",
                    user_name,
                    user_display_name,
                    credential_data,
                    created_at AS "created_at: OffsetDateTime",
                    updated_at AS "updated_at: OffsetDateTime",
                    last_used_at AS "last_used_at?: OffsetDateTime"
                FROM webauthn_credentials
                WHERE user_id = ?
                ORDER BY created_at DESC
            "#,
            user_id
        )
        .fetch_all(self.pool)
        .await
        .resolve()
    }

    pub async fn get_webauthn_credential_by_id(
        &self,
        credential_id: i64,
    ) -> DataBaseResult<Option<WebauthnCredential>> {
        query_as!(
            WebauthnCredential,
            r#"
                SELECT
                    id,
                    user_id,
                    user_unique_uuid AS "user_unique_uuid: Uuid",
                    user_name,
                    user_display_name,
                    credential_data,
                    created_at AS "created_at: OffsetDateTime",
                    updated_at AS "updated_at: OffsetDateTime",
                    last_used_at AS "last_used_at?: OffsetDateTime"
                FROM webauthn_credentials
                WHERE id = ?
            "#,
            credential_id
        )
        .fetch_optional(self.pool)
        .await
        .resolve()
    }

    pub async fn update_webauthn_last_used(&self, credential_id: i64) -> DataBaseResult<()> {
        query!(
            r#"
                UPDATE webauthn_credentials
                SET last_used_at = strftime('%s', 'now'),
                    updated_at = strftime('%s', 'now')
                WHERE id = ?
            "#,
            credential_id
        )
        .execute(self.pool)
        .await
        .resolve()?;
        Ok(())
    }

    async fn list_user_available_methods<E>(
        &self,
        user_id: i64,
        executor: &mut E,
    ) -> DataBaseResult<Vec<MFAAuthMethod>>
    where
        for<'e> &'e mut E: Executor<'e, Database = Sqlite>,
    {
        let totp_count = query_scalar!(
            // language=sql
            "SELECT COUNT(1) FROM totp_credentials WHERE user_id = ?",
            user_id
        )
        .fetch_one(&mut *executor)
        .await
        .resolve()?;
        let webauthn_count = query_scalar!(
            // language=sql
            "SELECT COUNT(1) FROM webauthn_credentials WHERE user_id = ?",
            user_id
        )
        .fetch_one(&mut *executor)
        .await
        .resolve()?;
        let mut methods = Vec::with_capacity(2);
        if totp_count > 0 {
            methods.push(MFAAuthMethod::Totp);
        }
        if webauthn_count > 0 {
            methods.push(MFAAuthMethod::Webauthn);
        }
        Ok(methods)
    }

    pub async fn get_mfa_infos(&self, user_ids: &[i64]) -> DataBaseResult<Vec<MfaInfo>> {
        let mut tx = self.pool.begin().await?;
        let mut res = Vec::with_capacity(user_ids.len());
        for &user_id in user_ids {
            let settings = self.get_mfa_info(user_id, &mut *tx).await?;
            let available_methods = self.list_user_available_methods(user_id, &mut *tx).await?;
            let mfa_info = match settings {
                Some(s) => MfaInfo {
                    user_id: s.user_id,
                    mfa_enabled: s.mfa_enabled,
                    updated_at: s.updated_at,
                    available_methods,
                },
                None => MfaInfo {
                    user_id,
                    mfa_enabled: false,
                    updated_at: OffsetDateTime::now_utc(),
                    available_methods,
                },
            };
            res.push(mfa_info);
        }
        Ok(res)
    }
}
