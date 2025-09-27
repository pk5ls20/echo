use crate::models::resource::ResourceTarget;
use crate::models::resource::{
    ResourceItemRawInfo, ResourceItemWithRefRaw, ResourceReferenceInner,
};
use crate::models::{Change, DiffRef};
use crate::services::states::db::{DataBaseResult, SqliteBaseResultExt};
use sqlx::{Executor, Sqlite, query};
use uuid::Uuid;

pub struct ResourceRepo<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    pub inner: &'a mut E,
}

impl<'a, E> ResourceRepo<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    pub async fn add_resource(
        &mut self,
        res_item_inner: ResourceItemRawInfo,
    ) -> DataBaseResult<i64> {
        let result = query!(
            "INSERT INTO resources (uploader_id, res_name, res_uuid, res_ext) VALUES (?, ?, ?, ?)",
            res_item_inner.uploader_id,
            res_item_inner.res_name,
            res_item_inner.res_uuid,
            res_item_inner.res_ext
        )
        .execute(&mut *self.inner)
        .await
        .resolve()?;
        let res_id = result.last_insert_rowid();
        Ok(res_id)
    }

    pub(in crate::services) async fn update_resource(
        &mut self,
        ref_diff: DiffRef<'_, ResourceReferenceInner>,
    ) -> DataBaseResult<()> {
        for diff in ref_diff.iter() {
            match diff.kind {
                Change::Added => {
                    query!(
                        "INSERT INTO resource_references (res_id, target_id, target_type) VALUES ($1, $2, $3)",
                        diff.value.res_id,
                        diff.value.target_id,
                        diff.value.target_type
                    )
                    .execute(&mut *self.inner)
                    .await
                    .resolve()?;
                }
                Change::Removed => {
                    query!(
                        "DELETE FROM resource_references WHERE res_id = $1 AND target_id = $2 AND target_type = $3",
                        diff.value.res_id,
                        diff.value.target_id,
                        diff.value.target_type
                    )
                    .execute(&mut *self.inner)
                    .await
                    .resolve()?;
                }
            }
        }
        Ok(())
    }

    pub(in crate::services) async fn delete_resources_batch(
        &mut self,
        res_ids: &[ResourceReferenceInner],
    ) -> DataBaseResult<()> {
        for res in res_ids {
            query!(
                "DELETE FROM resource_references WHERE res_id = $1 AND target_id = $2 AND target_type = $3",
                res.res_id,
                res.target_id,
                res.target_type
            )
            .execute(&mut *self.inner)
            .await
            .resolve()?;
        }
        Ok(())
    }

    pub(in crate::services) async fn get_resource_by_id_batch(
        &mut self,
        res_ids: &[i64],
    ) -> DataBaseResult<Vec<ResourceItemWithRefRaw>> {
        if res_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids_json = serde_json::to_string(res_ids)?;
        let rows = query!(
            r#"
                WITH input(id, ord) AS (
                    SELECT CAST(value AS INTEGER) AS id,
                           CAST(key AS INTEGER) AS ord
                    FROM json_each(?)
                )
                SELECT
                    r.id,
                    r.uploader_id,
                    r.res_name,
                    r.res_uuid AS "res_uuid: Uuid",
                    r.res_ext,
                    rr.target_id,
                    rr.target_type AS "target_type: ResourceTarget"
                FROM input i
                JOIN resources r           ON r.id = i.id
                JOIN resource_references rr ON rr.res_id = r.id
                ORDER BY i.ord
            "#,
            ids_json
        )
        .fetch_all(&mut *self.inner)
        .await
        .resolve()?;
        let out = rows
            .into_iter()
            .map(|row| ResourceItemWithRefRaw {
                id: row.id,
                info: ResourceItemRawInfo {
                    uploader_id: row.uploader_id,
                    res_name: row.res_name,
                    res_uuid: row.res_uuid,
                    res_ext: row.res_ext,
                },
                target_id: row.target_id,
                target_type: row.target_type,
            })
            .collect();
        Ok(out)
    }
}
