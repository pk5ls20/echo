use crate::models::users::{Role, UserInternal, UserRow, UserRowOptional};
use crate::services::states::db::permission::PermissionRepo;
use crate::services::states::db::{DataBaseResult, SqliteBaseResultExt};
use sqlx::{SqlitePool, query, query_as, query_scalar};
use time::OffsetDateTime;

pub struct UserRepo<'a> {
    pool: &'a SqlitePool,
}

impl<'a> UserRepo<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_user_count(&self) -> DataBaseResult<i64> {
        query_scalar!(
            // language=sql
            "SELECT COUNT(*) AS 'count: i64' FROM users"
        )
        .fetch_one(self.pool)
        .await
        .resolve()
    }

    pub async fn add_user(
        &self,
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
        .execute(self.pool)
        .await
        .resolve()
        .map(|result| result.last_insert_rowid())
    }

    pub async fn check_user_exists(&self, user_ids: &[i64]) -> DataBaseResult<Option<i64>> {
        for &id in user_ids {
            let exists = query_scalar!(
                // language=sql
                "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?) AS 'exists: bool'",
                id
            )
            .fetch_one(self.pool)
            .await
            .resolve()?;
            if !exists {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    pub(in crate::services) async fn update_user(
        &self,
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
        .execute(self.pool)
        .await
        .resolve()?;
        Ok(())
    }

    pub async fn remove_user_by_username(&self, username: &str) -> DataBaseResult<()> {
        query!("DELETE FROM users WHERE username = $1", username,)
            .fetch_one(self.pool)
            .await
            .resolve()?;
        Ok(())
    }

    pub(in crate::services) async fn remove_user_by_id(&self, user_id: i64) -> DataBaseResult<()> {
        query!("DELETE FROM users WHERE id = $1", user_id,)
            .execute(self.pool)
            .await
            .resolve()?;
        Ok(())
    }

    async fn build_user_internal_from_row(&self, row: UserRow) -> DataBaseResult<UserInternal> {
        let pm_repo = PermissionRepo::new(self.pool);
        let permissions = pm_repo
            .combined_query_user_permission(row.id, &row.role)
            .await?;
        Ok(UserInternal {
            inner: row,
            permissions,
        })
    }

    pub async fn query_user_by_username(
        &self,
        username: &str,
    ) -> DataBaseResult<Option<UserInternal>> {
        let row_opt = query_as!(
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
        .fetch_optional(self.pool)
        .await
        .resolve()?;
        match row_opt {
            Some(row) => self.build_user_internal_from_row(row).await.map(Some),
            None => Ok(None),
        }
    }

    pub(in crate::services) async fn query_user_by_id(
        &self,
        user_id: i64,
    ) -> DataBaseResult<Option<UserInternal>> {
        let row_opt = query_as!(
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
        .fetch_optional(self.pool)
        .await
        .resolve()?;
        match row_opt {
            Some(row) => self.build_user_internal_from_row(row).await.map(Some),
            None => Ok(None),
        }
    }
}
