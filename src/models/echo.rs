use crate::models::users::{Role, User};
use crate::services::states::db::PageQueryCursor;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use time::OffsetDateTime;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct EchoFullViewRaw {
    pub id: i64,
    pub user_id: i64,
    pub content: String,
    pub fav_count: i64,
    pub is_private: bool,
    #[serde(with = "time::serde::timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp")]
    pub last_modified_at: OffsetDateTime,
    #[sqlx(rename = "permission_ids_json")]
    pub permission_ids: Json<Vec<i64>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EchoPermission {
    Public,
    WithPermissions { permissions: Vec<i64> },
    Private,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Echo {
    pub id: i64,
    pub user_id: i64,
    pub content: Option<String>,
    pub fav_count: i64,
    pub permission: EchoPermission,
    #[serde(with = "time::serde::timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp")]
    pub last_modified_at: OffsetDateTime,
}

impl Echo {
    pub fn has_permission(&self, current_user_info: &User) -> bool {
        let (&id, &role, permission_ids) = (
            &current_user_info.id,
            &current_user_info.role,
            &current_user_info.permission_ids,
        );
        match &self.permission {
            EchoPermission::Public => true,
            EchoPermission::Private => id == self.user_id || role == Role::Admin,
            EchoPermission::WithPermissions { permissions } => {
                permissions.iter().all(|pid| permission_ids.contains(pid))
            }
        }
    }
}

impl PageQueryCursor for Echo {
    fn cursor_field(&self) -> i64 {
        self.id
    }
}

impl From<EchoFullViewRaw> for Echo {
    fn from(raw: EchoFullViewRaw) -> Self {
        Self {
            id: raw.id,
            user_id: raw.user_id,
            content: Some(raw.content),
            fav_count: raw.fav_count,
            permission: match (raw.is_private, raw.permission_ids.0.is_empty()) {
                (true, _) => EchoPermission::Private,
                (false, true) => EchoPermission::Public,
                (false, false) => EchoPermission::WithPermissions {
                    permissions: raw.permission_ids.0,
                },
            },
            created_at: raw.created_at,
            last_modified_at: raw.last_modified_at,
        }
    }
}
