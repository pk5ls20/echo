use crate::get_batch_tuple;
use crate::models::DiffOwned;
use crate::models::api::prelude::*;
use crate::models::dyn_setting::{AllowMimeTypes, MaxFileSize, UploadChunkSize};
use crate::models::resource::{
    ResourceItemRaw, ResourceItemRawInner, ResourceReferenceInner, ResourceUploadHeader,
    UploadCreateReqMetaInfo,
};
use crate::models::session::BasicAuthData;
use crate::models::users::Role;
use crate::services::hybrid_cache::HybridCacheService;
use crate::services::states::EchoState;
use crate::services::states::db::DataBaseError;
use crate::services::upload_tracker::{
    ResourceUploadLimits, ResourceUploadProtocol, UploadTrackerService,
};
use axum::Json;
use axum::body::Body;
use axum::extract::{Query, State};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::io;
use std::sync::Arc;
use tokio_util::codec::FramedRead;
use tokio_util::io::StreamReader;
use uuid::Uuid;

pub type ResourceRouterState = State<(
    Arc<EchoState>,
    Arc<UploadTrackerService>,
    Arc<HybridCacheService>,
)>;

#[derive(Debug, Deserialize)]
pub struct UploadCreateReq {
    pub file_meta: UploadCreateReqMetaInfo,
}

#[derive(Debug, Serialize)]
pub struct UploadCreateResp {
    pub upload_session_uuid: Uuid,
    pub chunk_size: u32,
}

pub async fn upload_create(
    State((_, upload_tracker, cache)): ResourceRouterState,
    Json(req): Json<UploadCreateReq>,
) -> ApiResult<Json<GeneralResponse<UploadCreateResp>>> {
    let meta = &req.file_meta;
    let (max_file_size, upload_chunk_size, allow_mine_types) = get_batch_tuple!(
        cache.dyn_settings,
        MaxFileSize,
        UploadChunkSize,
        AllowMimeTypes
    )
    .map_err(|e| internal!(e, "Failed to get dynamic settings"))?;
    if meta.file_size == 0 || meta.file_size > max_file_size {
        return Err(bad_request!("Invalid file size"));
    }
    if let Some(allow) = &allow_mine_types
        && !allow.iter().any(|m| m.as_ref() == meta.file_mime_type)
    {
        return Err(bad_request!("Mime type not allowed"));
    }
    if meta.file_sha1.len() != 40 || hex::decode(&meta.file_sha1).is_err() {
        return Err(bad_request!("Invalid file sha1 format"));
    }
    let session_id = Uuid::new_v4();
    upload_tracker
        .init_tracker(req.file_meta, upload_chunk_size, &session_id)
        .await
        .map_err(|e| internal!(&e, "Failed to initialize upload tracker"))?;
    Ok(general_json_res!(
        "Upload session created",
        UploadCreateResp {
            upload_session_uuid: session_id,
            chunk_size: upload_chunk_size.into(),
        }
    ))
}

#[derive(Debug, Deserialize)]
pub struct UploadChunkQuery {
    pub session_uuid: Uuid,
}

pub async fn upload_chunk(
    State((state, upload_tracker, cache)): ResourceRouterState,
    Query(q): Query<UploadChunkQuery>,
    body: Body,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let upload_chunk_size = get_batch_tuple!(cache.dyn_settings, UploadChunkSize)
        .map_err(|e| internal!(e, "Failed to get upload chunk size"))?
        .0;
    let codec = ResourceUploadProtocol::new(ResourceUploadLimits {
        flush_stream_size: state.config.resource.flush_stream_size,
        max_head_size: ResourceUploadHeader::max_size(),
        max_body_size: upload_chunk_size.into(),
    });
    let data_stream = body
        .into_data_stream()
        .map(|res| res.map_err(io::Error::other));
    let reader = StreamReader::new(data_stream);
    let mut stream = FramedRead::new(reader, codec);
    let (_, tracker) = upload_tracker
        .get_tracker(&q.session_uuid)
        .await
        .ok_or(bad_request!("Upload session not found"))?;
    tracker
        .accept_chunk_stream(&mut stream)
        .await
        .map_err(|e| internal!(&e, "Failed to accept chunk stream for upload session"))?;
    Ok(general_json_res!("Chunk uploaded successfully"))
}

#[derive(Debug, Deserialize)]
pub struct UploadCommitReq {
    pub upload_session_uuid: Uuid,
}

#[derive(Debug, Serialize)]
pub struct UploadCommitRes {
    pub res_id: i64,
}

pub async fn upload_commit(
    current_user_info: BasicAuthData,
    State((state, upload_tracker, _)): ResourceRouterState,
    Json(req): Json<UploadCommitReq>,
) -> ApiResult<Json<GeneralResponse<UploadCommitRes>>> {
    // Pessimistic lock
    let tracker = upload_tracker
        .remove_tracker(&req.upload_session_uuid)
        .await
        .ok_or(bad_request!("Upload session not found"))?;
    // Force flush to avoid cloned arc
    state.cache.run_pending_upload_tracker_session_tasks().await;
    match Arc::try_unwrap(tracker) {
        Ok(mut tracker) => {
            tracker
                .merge()
                .await
                .map_err(|e| internal!(&e, "Failed to merge upload chunks"))?;
            let (file_name, file_ext) = tracker
                .commit()
                .await
                .map_err(|e| internal!(&e, "Failed to commit upload session!"))?;
            let res_id = state
                .db
                .resources()
                .add_resource(ResourceItemRawInner {
                    uploader_id: current_user_info.user_id,
                    res_name: file_name,
                    res_uuid: req.upload_session_uuid, // TODO: use Neko's UUID algo
                    res_ext: file_ext,
                })
                .await
                .map_err(|e| internal!(&e, "Failed to add resource to database"))?;
            Ok(general_json_res!(
                "Upload committed successfully",
                UploadCommitRes { res_id }
            ))
        }
        Err(tracker) => {
            let (strong, weak) = (Arc::strong_count(&tracker), Arc::weak_count(&tracker));
            // rollback
            upload_tracker
                .set_tracker(&req.upload_session_uuid, tracker, None)
                .await;
            Err(internal!(format!(
                "Failed to get mutable reference to upload tracker! (strong: {}, weak: {})",
                strong, weak
            )))
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateResourceReq {
    pub diff: DiffOwned<ResourceReferenceInner>,
}

// TODO: allow everyone & permission check (yay we also need uploader_id)
pub async fn update_resource(
    current_user_info: BasicAuthData,
    State((state, _, cache)): ResourceRouterState,
    Json(req): Json<UpdateResourceReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("You are not allowed to update resources"));
    }
    state
        .db
        .resources()
        .update_resource((&req.diff).into())
        .await
        .map_err(|e| internal!(e, "Failed to update resource"))?;
    Ok(general_json_res!("Resource updated successfully"))
}

#[derive(Debug, Deserialize)]
pub struct DeleteResourceQuery {
    pub resources: Vec<ResourceReferenceInner>,
}

// TODO: allow everyone & permission check (yay we also need uploader_id)
pub async fn delete_resource(
    current_user_info: BasicAuthData,
    State((state, _, cache)): ResourceRouterState,
    Query(req): Query<DeleteResourceQuery>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("You are not allowed to update resources"));
    }
    state
        .db
        .resources()
        .delete_resources_batch(&req.resources)
        .await
        .map_err(|e| internal!(e, "Failed to delete resource"))?;
    Ok(general_json_res!("Resource deleted successfully"))
}

#[derive(Debug, Deserialize)]
pub struct GetResourceByIdsReq {
    pub res_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
pub struct GetResourceByIdsRes {
    pub list: Vec<ResourceItemRaw>,
}

// TODO: allow everyone & permission check (yay we also need uploader_id)
pub async fn get_resource_by_ids(
    current_user_info: BasicAuthData,
    State((state, _, cache)): ResourceRouterState,
    Json(req): Json<GetResourceByIdsReq>,
) -> ApiResult<Json<GeneralResponse<GetResourceByIdsRes>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("You are not allowed to update resources"));
    }
    let list = state
        .db
        .resources()
        .get_resource_by_id_batch(&req.res_ids)
        .await
        .map_err(|e| match e {
            DataBaseError::RowNotFound(_) => bad_request!("Resource not found"),
            _ => internal!(e, "Failed to get resource by id from database"),
        })?;
    Ok(general_json_res!(
        "Resource info fetched successfully",
        GetResourceByIdsRes { list }
    ))
}
