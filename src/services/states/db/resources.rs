use crate::models::resource::{ResourceItemRaw, ResourceItemRawInner, ResourceReferenceInner};
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
        res_item_inner: ResourceItemRawInner,
    ) -> DataBaseResult<i64> {
        let result = query!(
            "INSERT INTO resources (uploader_id, res_name, res_uuid, res_ext) VALUES (?, ?, ?, ?)",
            res_item_inner.uploader_id,
            res_item_inner.res_name,
            res_item_inner.res_uuid,
            res_item_inner.uploader_id
        )
        .execute(&mut *self.inner)
        .await
        .resolve()?;
        let res_id = result.last_insert_rowid();
        Ok(res_id)
    }

    pub async fn add_resource_ref(
        &mut self,
        res_ref: ResourceReferenceInner,
    ) -> DataBaseResult<()> {
        query!(
            "INSERT INTO resource_references (res_id, target_id, target_type) VALUES ($1, $2, $3)",
            res_ref.res_id,
            res_ref.target_id,
            res_ref.target_type
        )
        .execute(&mut *self.inner)
        .await
        .resolve()?;
        Ok(())
    }

    pub async fn update_resource(
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

    pub async fn delete_resources_batch(
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

    pub async fn get_resource_by_id_batch(
        &mut self,
        res_ids: &[i64],
    ) -> DataBaseResult<Vec<ResourceItemRaw>> {
        let ids_json = serde_json::to_string(res_ids)?;
        let rows = query!(
            r#"
                SELECT r.id,
                       r.uploader_id,
                       r.res_name,
                       r.res_uuid as "res_uuid: Uuid",
                       r.res_ext
                FROM json_each(?) AS je
                JOIN resources AS r ON r.id = je.value
                ORDER BY je.rowid
            "#,
            ids_json
        )
        .fetch_all(&mut *self.inner)
        .await
        .resolve()?;
        let out = rows
            .into_iter()
            .map(|r| ResourceItemRaw {
                id: r.id,
                inner: ResourceItemRawInner {
                    uploader_id: r.uploader_id,
                    res_name: r.res_name,
                    res_uuid: r.res_uuid,
                    res_ext: r.res_ext,
                },
            })
            .collect();
        Ok(out)
    }
}
