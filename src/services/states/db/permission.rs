use crate::models::permission::{Permission, RawUserPermissionRow, UserAssignedPermission};
use crate::models::users::Role;
use crate::services::states::db::{
    DataBaseResult, PageQueryBinder, PageQueryResult, SqliteBaseResultExt,
};
use ahash::HashSet;
use sqlx::{SqlitePool, query, query_as};
use time::OffsetDateTime;

pub struct PermissionRepo<'a> {
    pool: &'a SqlitePool,
}

impl<'a> PermissionRepo<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn add_permission(&self, pm_desc: &str, pm_color: i64) -> DataBaseResult<()> {
        query!(
            "INSERT INTO permissions (description, color) VALUES (?, ?)",
            pm_desc,
            pm_color,
        )
        .execute(self.pool)
        .await
        .resolve()?;
        Ok(())
    }

    pub async fn modify_permission(
        &self,
        pm_id: i64,
        pm_desc: &str,
        pm_color: i64,
    ) -> DataBaseResult<()> {
        query!(
            "UPDATE permissions SET description = ?, color = ? WHERE id = ?",
            pm_desc,
            pm_color,
            pm_id,
        )
        .execute(self.pool)
        .await
        .resolve()?;
        Ok(())
    }

    pub async fn delete_permission(&self, pm_id: i64) -> DataBaseResult<()> {
        query!("DELETE FROM permissions WHERE id = ?", pm_id,)
            .execute(self.pool)
            .await
            .resolve()?;
        Ok(())
    }

    pub(in crate::services) async fn grant_user_permission(
        &self,
        user_id: i64,
        assigner_id: i64,
        permission_ids: &[i64],
        exp_time: Option<OffsetDateTime>,
    ) -> DataBaseResult<()> {
        let mut tx = self.pool.begin().await?;
        let exp_time_unix_timestamp = exp_time.map(|t| t.unix_timestamp());
        for &permission_id in permission_ids {
            query!(
                r#"
                INSERT INTO user_permissions (user_id, permission_id, assigner_id, exp_time)
                VALUES (?, ?, ?, ?)
            "#,
                user_id,
                permission_id,
                assigner_id,
                exp_time_unix_timestamp
            )
            .execute(&mut *tx)
            .await
            .resolve()?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn combined_query_user_permission(
        &self,
        user_id: i64,
        role: &Role,
    ) -> DataBaseResult<HashSet<Permission>> {
        match role {
            Role::Admin => self.list_all_permissions().await,
            _ => self.query_basic_user_owned_permission(user_id).await,
        }
    }

    async fn query_basic_user_owned_permission(
        &self,
        user_id: i64,
    ) -> DataBaseResult<HashSet<Permission>> {
        let perms = query_as!(
            Permission,
            r#"
                SELECT
                  p.id,
                  p.description,
                  p.color
                FROM permissions AS p
                JOIN user_permissions AS v ON p.id = v.permission_id
                WHERE v.user_id = ? AND v.active = 1
            "#,
            user_id
        )
        .fetch_all(self.pool)
        .await
        .resolve()?;
        Ok(perms.into_iter().collect())
    }

    async fn list_all_permissions(&self) -> DataBaseResult<HashSet<Permission>> {
        let perms = query_as!(Permission, "SELECT id, description, color FROM permissions")
            .fetch_all(self.pool)
            .await
            .resolve()?;
        Ok(perms.into_iter().collect())
    }

    pub async fn get_permissions_record_page(
        &self,
        page: PageQueryBinder,
    ) -> DataBaseResult<PageQueryResult<UserAssignedPermission>> {
        let PageQueryResult {
            items,
            has_more,
            next_cursor,
        } = page
            .query_page_ctx(|pq| async move {
                query_as!(
                    RawUserPermissionRow,
                    r#"
                        SELECT
                            p.id AS "permission_id: _",
                            p.description,
                            p.color,
                            up.id AS "record_id: _",
                            up.assigner_id,
                            up.assigned_at AS "assigned_at: _",
                            up.exp_time AS "exp_time: _",
                            up.revoked_at AS "revoked_at: _",
                            up.active AS "active: _"
                        FROM permissions AS p
                        JOIN user_permissions AS up ON p.id = up.permission_id
                        WHERE up.id > ?
                        LIMIT ?
                    "#,
                    pq.start_after,
                    pq.limit,
                )
                .fetch_all(self.pool)
                .await
            })
            .await?;
        let items = items
            .into_iter()
            .map(|r| UserAssignedPermission {
                permission: Permission {
                    id: r.permission_id,
                    description: r.description,
                    color: r.color,
                },
                id: r.record_id,
                exp_time: r.exp_time,
                assigner_id: r.assigner_id,
                assigned_at: r.assigned_at,
                revoked_at: r.revoked_at,
                active: r.active,
            })
            .collect();
        Ok(PageQueryResult {
            items,
            has_more,
            next_cursor,
        })
    }

    pub(in crate::services) async fn revoke_permissions(
        &self,
        user_id: i64,
        permission_ids: &[i64],
    ) -> DataBaseResult<()> {
        let tx = self.pool.begin().await?;
        for &pid in permission_ids {
            query!(
                r#"
                UPDATE user_permissions
                SET revoked_at = CURRENT_TIMESTAMP
                WHERE user_id = ? AND permission_id = ?
            "#,
                user_id,
                pid
            )
            .execute(self.pool)
            .await
            .resolve()?;
        }
        tx.commit().await?;
        Ok(())
    }
}
