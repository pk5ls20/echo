use crate::models::invite_code::InviteCodeRaw;
use crate::services::states::db::{
    DataBaseResult, PageQueryBinder, PageQueryResult, SqliteBaseResultExt, SqliteQueryResultExt,
};
use sqlx::{Executor, Sqlite, query, query_as};

pub struct InviteCodeRepo<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    pub inner: &'a mut E,
}

impl<'a, E> InviteCodeRepo<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    pub async fn insert_invite_code(
        &mut self,
        code: &str,
        issued_by_user_id: i64,
        exp_time: i64,
    ) -> DataBaseResult<i64> {
        let res = query!(
            "INSERT INTO invite_codes (code, issued_by, exp_time) VALUES (?, ?, ?)",
            code,
            issued_by_user_id,
            exp_time
        )
        .execute(&mut *self.inner)
        .await
        .resolve()?;
        Ok(res.last_insert_rowid())
    }

    pub async fn get_invite_code_by_code<T>(
        &mut self,
        code: T,
    ) -> DataBaseResult<Option<InviteCodeRaw>>
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
                    exp_time as "exp_time: _",
                    is_used as "is_used: _",
                    used_by,
                    used_at as "used_at: _"
                FROM invite_codes
                WHERE code = ?
            "#,
            code
        )
        .fetch_optional(&mut *self.inner)
        .await
        .resolve()?;
        Ok(res)
    }

    pub async fn revoke_invite_code<T>(&mut self, issuer: &[(T, i64)]) -> DataBaseResult<()>
    where
        T: AsRef<str>,
    {
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
            .execute(&mut *self.inner)
            .await
            .resolve_affected()?;
        }
        Ok(())
    }

    pub async fn list_invite_codes_page(
        &mut self,
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
            .fetch_all(&mut *self.inner)
            .await
        })
        .await
    }
}
