use crate::models::permission::Permission;
use ahash::HashSet;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::BTreeSet;
use time::OffsetDateTime;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[repr(u8)]
pub enum Role {
    /// Administrators have full granular permissions and can assign granular permissions to regular users
    Admin = 1,
    /// Users may can choose to post messages and forward messages, but obtaining refined permissions is passive.
    User = 2,
    /// Guests may only be able to browse posts.
    Guest = 3,
}

#[derive(Debug, FromRow)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub role: Role,
    pub created_at: OffsetDateTime,
    pub avatar_res_id: Option<i64>,
}

#[derive(Debug)]
pub struct UserRowOptional {
    pub id: i64,
    pub username: Option<String>,
    pub password_hash: Option<String>,
    pub role: Option<Role>,
    pub avatar_res_id: Option<Option<i64>>,
}

#[derive(Debug)]
pub struct UserInternal {
    pub inner: UserRow,
    pub permissions: HashSet<Permission>,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: Role,
    #[serde(with = "time::serde::timestamp")]
    pub created_at: OffsetDateTime,
    pub permission_ids: BTreeSet<i64>,
    pub avatar_res_id: Option<i64>,
}

impl UserInternal {
    pub fn permission_ids(&self) -> BTreeSet<i64> {
        self.permissions.iter().map(|p| p.id).collect()
    }

    pub fn into_public(self) -> User {
        User {
            permission_ids: self.permission_ids(),
            id: self.inner.id,
            username: self.inner.username,
            role: self.inner.role,
            created_at: self.inner.created_at,
            avatar_res_id: self.inner.avatar_res_id,
        }
    }
}
