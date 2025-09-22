use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: DeserializeOwned"))]
pub struct BaseSession<T>
where
    T: Debug + Clone + Serialize + DeserializeOwned,
{
    pub session_id: Uuid,
    #[serde(with = "time::serde::timestamp")]
    pub create_at: OffsetDateTime,
    pub inner: T,
}

impl<T> BaseSession<T>
where
    T: Debug + Clone + Serialize + DeserializeOwned,
{
    pub fn new(session_id: Uuid, inner: T) -> Self {
        Self {
            session_id,
            create_at: OffsetDateTime::now_utc(),
            inner,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicAuthData {
    pub user_id: i64,
}

impl BasicAuthData {
    pub fn new(user_id: i64) -> Self {
        Self { user_id }
    }
}

pub type BasicAuthSessionData = BaseSession<BasicAuthData>;

pub type CsrfAuthData = ();

pub type CsrfAuthSessionData = BaseSession<CsrfAuthData>;

pub type PreMfaAuthData = ();

pub type PreMfaAuthSessionData = BaseSession<PreMfaAuthData>;

pub type MfaAuthData = ();

pub type MfaAuthSessionData = BaseSession<MfaAuthData>;
