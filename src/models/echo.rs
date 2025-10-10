use crate::models::users::{Role, User};
use crate::services::states::db::PageQueryCursor;
use ahash::RandomState;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use std::hash::{BuildHasher, Hash, Hasher};
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
    pub permission_ids: Option<Json<Vec<i64>>>,
}

#[derive(Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
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
    #[cfg(test)]
    pub fn dummy_from_str(content: &str) -> Self {
        Self {
            id: rand::random(),
            user_id: 0,
            content: Some(content.to_string()),
            fav_count: 0,
            permission: EchoPermission::Public,
            created_at: OffsetDateTime::now_utc(),
            last_modified_at: OffsetDateTime::now_utc(),
        }
    }

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

    pub fn render_hash(&self) -> u64 {
        let mut h =
            RandomState::with_seeds(1887127636, 1496089152, 1496089150, 1804321245).build_hasher();
        self.id.hash(&mut h);
        self.permission.hash(&mut h);
        self.last_modified_at.hash(&mut h);
        h.finish()
    }
}

impl PageQueryCursor for Echo {
    fn cursor_field(&self) -> i64 {
        self.id
    }
}

impl From<EchoFullViewRaw> for Echo {
    fn from(raw: EchoFullViewRaw) -> Self {
        let pm_ids = raw.permission_ids.map(|id| id.0);
        Self {
            id: raw.id,
            user_id: raw.user_id,
            content: Some(raw.content),
            fav_count: raw.fav_count,
            permission: match (raw.is_private, pm_ids) {
                (true, _) => EchoPermission::Private,
                (false, None) => EchoPermission::Public,
                (false, Some(pm_ids)) => EchoPermission::WithPermissions {
                    permissions: pm_ids,
                },
            },
            created_at: raw.created_at,
            last_modified_at: raw.last_modified_at,
        }
    }
}
