use sqlx::FromRow;
use time::OffsetDateTime;

#[derive(Debug, FromRow)]
pub struct AuthTokenRaw {
    pub id: i64,
    pub user_id: i64,
    pub token: String,
    pub created_at: OffsetDateTime,
    pub exp_time: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
}
