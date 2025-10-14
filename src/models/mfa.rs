use crate::services::states::db::PageQueryCursor;
use ph::fmph;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::fmt::Debug;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[repr(u8)]
#[serde(rename_all = "lowercase")]
pub enum MFAOpType {
    Add = 1,
    Delete = 2,
    Auth = 3,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[repr(u8)]
#[serde(rename_all = "lowercase")]
pub enum MFAAuthMethod {
    Totp = 1,
    Webauthn = 2,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct MfaSettings {
    pub user_id: i64,
    pub mfa_enabled: bool,
    #[serde(with = "time::serde::timestamp")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug)]
pub struct MfaSettingsOptional {
    pub user_id: i64,
    pub mfa_enabled: Option<bool>,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct TotpCredential {
    pub id: i64,
    pub user_id: i64,
    pub totp_credential_data: Vec<u8>,
    #[serde(with = "time::serde::timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp::option")]
    pub last_used_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: DeserializeOwned"))]
pub struct WebauthnState<T>
where
    T: Debug + Serialize + DeserializeOwned,
{
    pub user_name: String,
    pub state: T,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct WebauthnCredential {
    pub id: i64,
    pub user_id: i64,
    pub user_unique_uuid: Uuid,
    pub user_name: String,
    pub user_display_name: Option<String>,
    pub credential_data: Vec<u8>,
    #[serde(with = "time::serde::timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp::option")]
    pub last_used_at: Option<OffsetDateTime>,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct MfaAuthLog {
    pub id: i64,
    pub user_id: i64,
    pub op_type: MFAOpType,
    pub auth_method: MFAAuthMethod,
    pub is_success: bool,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub credential_id: Option<i64>,
    pub error_message: Option<String>,
    #[serde(with = "time::serde::timestamp")]
    pub time: OffsetDateTime,
}

impl PageQueryCursor for MfaAuthLog {
    fn cursor_field(&self) -> i64 {
        self.id
    }
}

#[derive(Debug)]
pub struct NewTotpCredential {
    pub user_id: i64,
    pub totp_credential_data: Vec<u8>,
}

#[derive(Debug)]
pub struct NewWebauthnCredential {
    pub user_id: i64,
    pub user_unique_uuid: Uuid,
    pub user_name: String,
    pub user_display_name: Option<String>,
    pub credential_data: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewMfaAuthLog {
    pub user_id: i64,
    pub op_type: MFAOpType,
    pub info: NewMfaAuthLogInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewMfaAuthLogInfo {
    pub auth_method: MFAAuthMethod,
    pub is_success: bool,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub credential_id: Option<i64>,
    pub error_message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MfaInfo {
    pub user_id: i64,
    pub mfa_enabled: bool,
    #[serde(with = "time::serde::timestamp")]
    pub updated_at: OffsetDateTime,
    pub available_methods: Vec<MFAAuthMethod>,
}

pub struct WebAuthnKV {
    pub f: fmph::GOFunction,
    pub idx2id: Vec<i64>,
}
