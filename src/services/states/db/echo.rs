use crate::models::echo::{Echo, EchoFullViewRaw};
use crate::models::resource::ResourceTarget;
use crate::services::states::db::{
    DataBaseResult, PageQueryBinder, PageQueryResult, SqliteBaseResultExt,
};
use sqlx::types::Json;
use sqlx::{Executor, Sqlite, query, query_as, query_scalar};
use time::OffsetDateTime;

pub struct EchoRepo<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    pub inner: &'a mut E,
}

impl<'a, E> EchoRepo<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    async fn link_echo_res(&mut self, echo_id: i64, res_ids: &[i64]) -> DataBaseResult<()> {
        query!(
            "DELETE FROM resource_references WHERE target_id = ? AND target_type = ?",
            echo_id,
            ResourceTarget::Echo
        )
        .execute(&mut *self.inner)
        .await?;
        for res_id in res_ids {
            query!(
                "INSERT INTO resource_references (res_id, target_id, target_type) VALUES (?, ?, ?)",
                res_id,
                echo_id,
                ResourceTarget::Echo
            )
            .execute(&mut *self.inner)
            .await
            .resolve()?;
        }
        Ok(())
    }

    async fn link_echo_permission(
        &mut self,
        echo_id: i64,
        permission_ids: &[i64],
    ) -> DataBaseResult<()> {
        query!("DELETE FROM echo_permissions WHERE echo_id = ?", echo_id)
            .execute(&mut *self.inner)
            .await?; // prefer not resolve here
        for perm_id in permission_ids {
            query!(
                "INSERT INTO echo_permissions (echo_id, permission_id) VALUES (?, ?)",
                echo_id,
                perm_id
            )
            .execute(&mut *self.inner)
            .await
            .resolve()?;
        }
        Ok(())
    }

    pub async fn add_echo(
        &mut self,
        user_id: i64,
        new_content: &str,
        new_resource_ids: &[i64],
        permission_ids: &[i64],
        is_private: bool,
    ) -> DataBaseResult<i64> {
        let result = query!(
            "INSERT INTO echos (user_id, content, is_private) VALUES (?, ?, ?)",
            user_id,
            new_content,
            is_private
        )
        .execute(&mut *self.inner)
        .await
        .resolve()?;
        let new_echo_id = result.last_insert_rowid();
        self.link_echo_res(new_echo_id, new_resource_ids).await?;
        self.link_echo_permission(new_echo_id, permission_ids)
            .await?;
        Ok(new_echo_id)
    }

    pub async fn update_echo(
        &mut self,
        echo_id: i64,
        new_content: &str,
        new_resource_ids: &[i64],
        new_permission_ids: &[i64],
        is_private: bool,
    ) -> DataBaseResult<()> {
        query_scalar!(
            // language=sql
            "UPDATE echos SET content = ?, is_private = ? WHERE id = ? RETURNING id",
            new_content,
            echo_id,
            is_private
        )
        .fetch_one(&mut *self.inner)
        .await
        .resolve()?;
        self.link_echo_res(echo_id, new_resource_ids).await?;
        self.link_echo_permission(echo_id, new_permission_ids)
            .await?;
        Ok(())
    }

    pub async fn delete_echo(&mut self, echo_id: i64) -> DataBaseResult<()> {
        query_scalar!(
            // language=sql
            "DELETE FROM echos WHERE id = ?",
            echo_id
        )
        .fetch_one(&mut *self.inner)
        .await
        .resolve()?;
        self.link_echo_res(echo_id, &[]).await?;
        self.link_echo_permission(echo_id, &[]).await?;
        Ok(())
    }

    pub async fn query_echo_by_id(&mut self, echo_id: i64) -> DataBaseResult<Option<Echo>> {
        let row = query_as!(
            EchoFullViewRaw,
            r#"
                SELECT
                    e.id,
                    e.user_id,
                    e.content,
                    e.fav_count,
                    e.is_private AS "is_private: bool",
                    e.created_at AS "created_at: OffsetDateTime",
                    e.last_modified_at AS "last_modified_at: OffsetDateTime",
                    COALESCE(json_group_array(ep.permission_id), json('[]'))
                        AS "permission_ids: Json<Vec<i64>>"
                FROM echos AS e
                LEFT JOIN echo_permissions AS ep ON e.id = ep.echo_id
                WHERE e.id = ?
                GROUP BY e.id
            "#,
            echo_id
        )
        .fetch_optional(&mut *self.inner)
        .await?
        .map(Into::into);
        Ok(row)
    }

    pub async fn query_user_echo(
        &mut self,
        user_id: Option<i64>,
        page: PageQueryBinder,
    ) -> DataBaseResult<PageQueryResult<Echo>> {
        page.query_page_ctx(|pq| async move {
            let rows = query_as!(
                EchoFullViewRaw,
                r#"
                    SELECT
                      e.id,
                      e.user_id,
                      e.content,
                      e.fav_count,
                      e.is_private AS "is_private: bool",
                      e.created_at AS "created_at: OffsetDateTime",
                      e.last_modified_at AS "last_modified_at: OffsetDateTime",
                      COALESCE(
                        json_group_array(ep.permission_id) FILTER (WHERE ep.permission_id IS NOT NULL),
                        json('[]')
                      ) AS "permission_ids: Json<Vec<i64>>"
                    FROM echos AS e
                    LEFT JOIN echo_permissions AS ep ON e.id = ep.echo_id
                    WHERE (?1 IS NULL OR e.user_id = ?1) AND e.id > ?2
                    GROUP BY e.id
                    ORDER BY e.id
                    LIMIT ?3;
                "#,
                user_id,
                pq.start_after,
                pq.limit,
            )
            .fetch_all(&mut *self.inner)
            .await?;
            let items = rows.into_iter().map(Into::into).collect();
            Ok(items)
        })
        .await
    }
}
