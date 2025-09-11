use crate::models::token::AuthTokenRaw;
use crate::services::states::db::{DataBaseResult, SqliteBaseResultExt};
use sqlx::{SqlitePool, query, query_as};
use time::OffsetDateTime;

pub struct TokenRepo<'a> {
    pool: &'a SqlitePool,
}

impl<'a> TokenRepo<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert_user_token(
        &self,
        user_id: i64,
        token: String,
        exp_time: OffsetDateTime,
    ) -> DataBaseResult<AuthTokenRaw> {
        query_as!(
            AuthTokenRaw,
            r#"
                INSERT INTO auth_tokens (user_id, token, exp_time)
                VALUES (?, ?, ?)
                RETURNING
                    id, user_id, token,
                    created_at AS "created_at: OffsetDateTime",
                    exp_time AS "exp_time: OffsetDateTime",
                    last_used_at AS "last_used_at: _"
            "#,
            user_id,
            token,
            exp_time
        )
        .fetch_one(self.pool)
        .await
        .resolve()
    }

    pub async fn get_user_token(&self, user_id: i64) -> DataBaseResult<Vec<AuthTokenRaw>> {
        query_as!(
            AuthTokenRaw,
            r#"
                SELECT
                    id,
                    user_id,
                    token,
                    created_at AS "created_at: OffsetDateTime",
                    exp_time AS "exp_time: OffsetDateTime",
                    last_used_at AS "last_used_at: _"
                FROM auth_tokens
                WHERE user_id = ?
            "#,
            user_id
        )
        .fetch_all(self.pool)
        .await
        .resolve()
    }

    pub async fn invalidate_user_token(&self, user_id: i64, token: String) -> DataBaseResult<()> {
        query!(
            r#"
                UPDATE auth_tokens
                SET last_used_at = CURRENT_TIMESTAMP
                WHERE user_id = ? AND token = ?
            "#,
            user_id,
            token
        )
        .fetch_one(self.pool)
        .await
        .resolve()?;
        Ok(())
    }
}
