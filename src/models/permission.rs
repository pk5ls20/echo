use crate::services::states::db::PageQueryCursor;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, FromRow)]
pub struct Permission {
    pub id: i64,
    pub description: String,
    pub color: i64,
}

impl Permission {
    #[inline]
    pub const fn is_valid_color_i64(color: i64) -> bool {
        color >= 0 && color <= 0x00FFFFFF
    }
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct RawUserPermissionRow {
    pub permission_id: i64,
    pub description: String,
    pub color: i64,
    pub record_id: i64,
    #[serde(with = "time::serde::timestamp::option")]
    pub exp_time: Option<OffsetDateTime>,
    pub assigner_id: i64,
    #[serde(with = "time::serde::timestamp")]
    pub assigned_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp::option")]
    pub revoked_at: Option<OffsetDateTime>,
    pub active: bool,
}

impl PageQueryCursor for RawUserPermissionRow {
    fn cursor_field(&self) -> i64 {
        self.record_id
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserAssignedPermission {
    pub id: i64,
    pub permission: Permission,
    #[serde(with = "time::serde::timestamp::option")]
    pub exp_time: Option<OffsetDateTime>,
    pub assigner_id: i64,
    #[serde(with = "time::serde::timestamp")]
    pub assigned_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp::option")]
    pub revoked_at: Option<OffsetDateTime>,
    pub active: bool,
}
