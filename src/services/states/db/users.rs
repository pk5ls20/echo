use crate::models::users::{Role, UserRow, UserRowOptional};
use crate::services::states::db::{DataBaseResult, SqliteBaseResultExt};
use sqlx::{Executor, Sqlite, query, query_as, query_scalar};
use time::OffsetDateTime;

pub struct UsersRepo<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    pub inner: &'a mut E,
}

impl<'a, E> UsersRepo<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    pub async fn get_user_count(&mut self) -> DataBaseResult<i64> {
        query_scalar!(
            // language=sql
            "SELECT COUNT(*) AS 'count: i64' FROM users"
        )
        .fetch_one(&mut *self.inner)
        .await
        .resolve()
    }

    pub async fn add_user(
        &mut self,
        username: &str,
        password_hash: &str,
        role: Role,
    ) -> DataBaseResult<i64> {
        query!(
            "INSERT INTO users (username, password_hash, role) VALUES ($1, $2, $3)",
            username,
            password_hash,
            role,
        )
        .execute(&mut *self.inner)
        .await
        .resolve()
        .map(|result| result.last_insert_rowid())
    }

    pub async fn check_user_exists(&mut self, user_ids: &[i64]) -> DataBaseResult<Option<i64>> {
        for &id in user_ids {
            let exists = query_scalar!(
                // language=sql
                "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?) AS 'exists: bool'",
                id
            )
            .fetch_one(&mut *self.inner)
            .await
            .resolve()?;
            if !exists {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    pub(in crate::services) async fn update_user(
        &mut self,
        update_user: UserRowOptional,
    ) -> DataBaseResult<()> {
        query!(
            r#"
                UPDATE users
                SET username      = COALESCE($1, username),
                    password_hash = COALESCE($2, password_hash),
                    role          = COALESCE($3, role),
                    avatar_res_id = COALESCE($4, avatar_res_id)
                WHERE id = $5
            "#,
            update_user.username,
            update_user.password_hash,
            update_user.role,
            update_user.avatar_res_id,
            update_user.id,
        )
        .execute(&mut *self.inner)
        .await
        .resolve()?;
        Ok(())
    }

    pub async fn remove_user_by_username(&mut self, username: &str) -> DataBaseResult<()> {
        query!("DELETE FROM users WHERE username = $1", username,)
            .fetch_one(&mut *self.inner)
            .await
            .resolve()?;
        Ok(())
    }

    pub(in crate::services) async fn remove_user_by_id(
        &mut self,
        user_id: i64,
    ) -> DataBaseResult<()> {
        query!("DELETE FROM users WHERE id = $1", user_id,)
            .execute(&mut *self.inner)
            .await
            .resolve()?;
        Ok(())
    }

    pub async fn query_user_by_username(
        &mut self,
        username: &str,
    ) -> DataBaseResult<Option<UserRow>> {
        query_as!(
            UserRow,
            r#"
                SELECT
                  u.id,
                  u.username,
                  u.password_hash,
                  u.role AS "role: Role",
                  u.created_at AS "created_at: OffsetDateTime",
                  u.avatar_res_id
                FROM users AS u
                WHERE u.username = ?
            "#,
            username
        )
        .fetch_optional(&mut *self.inner)
        .await
        .resolve()
    }

    pub(in crate::services) async fn query_user_by_id(
        &mut self,
        user_id: i64,
    ) -> DataBaseResult<Option<UserRow>> {
        query_as!(
            UserRow,
            r#"
                SELECT
                  u.id,
                  u.username,
                  u.password_hash,
                  u.role AS "role: Role",
                  u.created_at AS "created_at: OffsetDateTime",
                  u.avatar_res_id
                FROM users AS u
                WHERE u.id = ?
            "#,
            user_id
        )
        .fetch_optional(&mut *self.inner)
        .await
        .resolve()
    }
}
