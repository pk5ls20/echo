use crate::services::states::db::PageQueryCursor;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct InviteCodeRaw {
    pub id: i64,
    pub code: String,
    pub issued_by: i64,
    #[serde(with = "time::serde::timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp")]
    pub exp_time: OffsetDateTime,
    pub is_used: bool,
    pub used_by: Option<i64>,
    #[serde(with = "time::serde::timestamp::option")]
    pub used_at: Option<OffsetDateTime>,
}

impl InviteCodeRaw {
    pub fn is_valid(&self) -> bool {
        !self.is_used && self.exp_time > OffsetDateTime::now_utc()
    }
}

impl PageQueryCursor for InviteCodeRaw {
    fn cursor_field(&self) -> i64 {
        self.id
    }
}
