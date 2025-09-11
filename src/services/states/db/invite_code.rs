use crate::models::invite_code::InviteCodeRaw;
use crate::services::states::db::{
    DataBaseResult, PageQueryBinder, PageQueryResult, SqliteBaseResultExt, SqliteQueryResultExt,
};
use sqlx::{SqlitePool, query, query_as};

pub struct InviteCodeRepo<'a> {
    pool: &'a SqlitePool,
}

impl<'a> InviteCodeRepo<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert_invite_code(
        &self,
        code: &str,
        issued_by_user_id: i64,
        exp_time: i64,
    ) -> DataBaseResult<i64> {
        let res = query!(
            r#"
                INSERT INTO invite_codes (code, issued_by, exp_time)
                VALUES (?, ?, ?)
            "#,
            code,
            issued_by_user_id,
            exp_time
        )
        .execute(self.pool)
        .await
        .resolve()?;
        Ok(res.last_insert_rowid())
    }

    pub async fn get_invite_code_by_code<T>(&self, code: T) -> DataBaseResult<Option<InviteCodeRaw>>
    where
        T: AsRef<str>,
    {
        let code = code.as_ref();
        let res = query_as!(
            InviteCodeRaw,
            r#"
                SELECT
                    id,
                    code,
                    issued_by,
                    created_at as "created_at: _",
                    exp_time   as "exp_time: _",
                    is_used    as "is_used: _",
                    used_by,
                    used_at    as "used_at: _"
                FROM invite_codes
                WHERE code = ?
            "#,
            code
        )
        .fetch_optional(self.pool)
        .await
        .resolve()?;
        Ok(res)
    }

    pub async fn revoke_invite_code<T>(&self, issuer: &[(T, i64)]) -> DataBaseResult<()>
    where
        T: AsRef<str>,
    {
        let mut tx = self.pool.begin().await.resolve()?;
        for (code, user_id) in issuer {
            let code = code.as_ref();
            query!(
                r#"
                    UPDATE invite_codes
                    SET is_used = 1, used_at = CURRENT_TIMESTAMP, used_by = ?
                    WHERE code = ?
                "#,
                user_id,
                code
            )
            .execute(&mut *tx)
            .await
            .resolve_affected()?;
        }
        tx.commit().await.resolve()?;
        Ok(())
    }

    pub async fn list_invite_codes_page(
        &self,
        page: PageQueryBinder,
    ) -> DataBaseResult<PageQueryResult<InviteCodeRaw>> {
        page.query_page_ctx(|pq| async move {
            query_as!(
                InviteCodeRaw,
                r#"
                    SELECT
                        id,
                        code,
                        issued_by,
                        created_at as "created_at: _",
                        exp_time as "exp_time: _",
                        is_used as "is_used: _",
                        used_by,
                        used_at as "used_at: _"
                    FROM invite_codes
                    WHERE id > ?
                    ORDER BY id
                    LIMIT ?
                "#,
                pq.start_after,
                pq.limit,
            )
            .fetch_all(self.pool)
            .await
        })
        .await
    }
}
