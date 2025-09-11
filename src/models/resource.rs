use prost::Message;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::fmt::Debug;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ResourceItemRawInner {
    pub uploader_id: i64,
    pub res_name: String,
    pub res_uuid: Uuid,
    pub res_ext: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ResourceItemRaw {
    pub id: i64,
    #[sqlx(flatten)]
    pub inner: ResourceItemRawInner,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[repr(u8)]
#[serde(rename_all = "lowercase")]
pub enum ResourceTarget {
    Echo = 1,
    Avatar = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ResourceReferenceInner {
    pub res_id: i64,
    pub target_id: i64,
    pub target_type: ResourceTarget,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ResourceReference {
    pub id: i64,
    #[sqlx(flatten)]
    pub inner: ResourceReferenceInner,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResourceUploadSession {
    pub file_name: Option<String>,
    pub file_mime_type: String,
    pub file_size: u64,
    pub file_sha1: String,
    pub chunk_size: u64,
}

#[derive(Eq, PartialEq, Hash, prost::Message)]
pub struct ResourceUploadHeader {
    #[prost(bytes = "vec", tag = "1")]
    pub upload_session_id: Vec<u8>,
    #[prost(uint64, tag = "2")]
    pub chunk_bytes_offset: u64,
    #[prost(uint32, tag = "3")]
    pub chunk_length: u32,
    #[prost(bytes = "vec", tag = "4")]
    pub chunk_sha1: Vec<u8>,
}

impl ResourceUploadHeader {
    pub fn max_size() -> u32 {
        let possible_max_header = ResourceUploadHeader {
            upload_session_id: vec![0; 16],
            chunk_bytes_offset: u64::MAX,
            chunk_length: u32::MAX,
            chunk_sha1: vec![0; 20],
        };
        possible_max_header.encoded_len() as u32 // SAFETY
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadCreateReqMetaInfo {
    pub file_name: String,
    pub file_mime_type: String,
    pub file_size: u64,
    pub file_sha1: String,
}
