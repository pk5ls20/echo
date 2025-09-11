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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreMfaAuthData {
    pub need_login_mfa: bool,
    pub passed_login_mfa: bool,
}

impl PreMfaAuthData {
    pub fn new(need_login_mfa: bool, passed_login_mfa: bool) -> Self {
        Self {
            need_login_mfa,
            passed_login_mfa,
        }
    }
}

pub type BasicAuthSessionData = BaseSession<BasicAuthData>;

pub type CsrfAuthData = ();

pub type CsrfAuthSessionData = BaseSession<CsrfAuthData>;

pub type PreMfaAuthSessionData = BaseSession<PreMfaAuthData>;

pub type MfaAuthData = ();

pub type MfaAuthSessionData = BaseSession<MfaAuthData>;
