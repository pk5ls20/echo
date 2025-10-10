use crate::models::api::prelude::*;
use crate::models::permission::{Permission, UserAssignedPermission};
use crate::models::session::BasicAuthData;
use crate::models::users::Role;
use crate::services::hybrid_cache::HybridCacheService;
use crate::services::states::EchoState;
use crate::services::states::db::{EchoDatabaseExecutor, PageQueryBinder, PageQueryResult};
use ahash::HashSet;
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use time::OffsetDateTime;

pub type PermissionRouterState = State<(Arc<EchoState>, Arc<HybridCacheService>)>;

#[derive(Debug, Deserialize)]
pub struct AddPermissionReq {
    pub description: String,
    pub color: i64,
}

#[derive(Debug, Serialize)]
pub struct AddPermissionRes {
    pub permission_id: i64,
}

pub async fn add_permission(
    current_user_info: BasicAuthData,
    State((state, cache)): PermissionRouterState,
    Json(req): Json<AddPermissionReq>,
) -> ApiResult<Json<GeneralResponse<AddPermissionRes>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("Only admin can add permissions"));
    }
    if !Permission::is_valid_color_i64(req.color) {
        return Err(bad_request!("Invalid color value"));
    }
    let permission_id = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.permission()
                .add_permission(&req.description, req.color)
                .await
        })
        .await
        .map_err(|e| bad_request!(e, "Failed to add permission"))?;
    Ok(general_json_res!(
        "Permission added",
        AddPermissionRes { permission_id }
    ))
}

pub async fn modify_permission(
    current_user_info: BasicAuthData,
    State((state, cache)): PermissionRouterState,
    Json(req): Json<Permission>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("Only admin can modify permissions"));
    }
    if !Permission::is_valid_color_i64(req.color) {
        return Err(bad_request!("Invalid color value"));
    }
    state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.permission()
                .modify_permission(req.id, &req.description, req.color)
                .await
        })
        .await
        .map_err(|e| bad_request!(e, "Failed to modify permission"))?;
    Ok(general_json_res!("Permission modified"))
}

pub async fn delete_permission(
    current_user_info: BasicAuthData,
    State((state, cache)): PermissionRouterState,
    Json(req): Json<Permission>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("Only admin can delete permissions"));
    }
    state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.permission().delete_permission(req.id).await
        })
        .await
        .map_err(|e| bad_request!(e, "Failed to delete permission"))?;
    Ok(general_json_res!("Permission deleted"))
}

#[derive(Debug, Deserialize)]
pub struct GrantPermissionReq {
    pub user_id: i64,
    pub permission_ids: Vec<i64>,
    #[serde(with = "time::serde::timestamp::option")]
    pub exp_time: Option<OffsetDateTime>,
}

// TODO: 1. Determine whether the user already possesses this permission (set processing).
pub async fn grant_permission(
    current_user_info: BasicAuthData,
    State((_, cache)): PermissionRouterState,
    Json(req): Json<GrantPermissionReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("Only admin can grant permissions"));
    }
    cache
        .users
        .grant_user_permission(
            req.user_id,
            current_user_info.user_id,
            &req.permission_ids,
            req.exp_time,
        )
        .await
        .map_err(|e| bad_request!(e, "Failed to grant permission"))?;
    Ok(general_json_res!("Permission granted"))
}

pub async fn get_permission_info(
    current_user_info: BasicAuthData,
    State((state, cache)): PermissionRouterState,
) -> ApiResult<Json<GeneralResponse<HashSet<Permission>>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    let permissions = state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.permission()
                .combined_query_user_permission(current_user_info.user_id, &current_user.role)
                .await
        })
        .await
        .map_err(|e| bad_request!(e, "Failed to get permissions"))?;
    Ok(general_json_res!(
        "Successfully retrieved permission info",
        permissions
    ))
}

#[derive(Debug, Deserialize)]
pub struct RevokePermissionReq {
    pub user_id: i64,
    pub permission_ids: Vec<i64>,
}

pub async fn revoke_permission(
    current_user_info: BasicAuthData,
    State((_, cache)): PermissionRouterState,
    Json(req): Json<RevokePermissionReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("Only admin can revoke permissions"));
    }
    cache
        .users
        .revoke_user_permission(req.user_id, &req.permission_ids)
        .await
        .map_err(|e| bad_request!(e, "Failed to revoke permissions"))?;
    Ok(general_json_res!("Permissions revoked"))
}

#[derive(Debug, Deserialize)]
pub struct GetPermissionRecordsReq {
    #[serde(flatten)]
    page_query: PageQueryBinder,
}

pub async fn get_permission_records(
    current_user_info: BasicAuthData,
    State((state, cache)): PermissionRouterState,
    Json(req): Json<GetPermissionRecordsReq>,
) -> ApiResult<Json<GeneralResponse<PageQueryResult<UserAssignedPermission>>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("Only admin can view permission records"));
    }
    let records = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.permission()
                .get_permissions_record_page(req.page_query)
                .await
        })
        .await
        .map_err(|e| bad_request!(e, "Failed to query permission records"))?;
    Ok(general_json_res!(
        "Successfully retrieved permission records",
        records
    ))
}
