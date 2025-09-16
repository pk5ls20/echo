use crate::models::api::prelude::*;
use crate::models::invite_code::InviteCodeRaw;
use crate::models::session::BasicAuthData;
use crate::models::users::Role;
use crate::services::hybrid_cache::HybridCacheService;
use crate::services::states::EchoState;
use crate::services::states::db::{EchoDatabaseExecutor, PageQueryBinder, PageQueryResult};
use axum::Json;
use axum::extract::State;
use rand::Rng;
use rand::distr::Alphanumeric;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use time::OffsetDateTime;

pub type InviteCodeRouterState = State<(Arc<EchoState>, Arc<HybridCacheService>)>;

#[derive(Debug, Deserialize)]
pub struct CreateInviteCodeReq {
    pub exp_seconds: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CreateInviteCodeRes {
    pub id: i64,
    pub code: String,
    #[serde(with = "time::serde::timestamp")]
    pub exp_time: OffsetDateTime,
}

pub async fn create_invite_code(
    current_user_info: BasicAuthData,
    State((state, cache)): InviteCodeRouterState,
    Json(req): Json<CreateInviteCodeReq>,
) -> ApiResult<Json<GeneralResponse<CreateInviteCodeRes>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!(
            "You are not allowed to create invitation codes"
        ));
    }
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let exp_seconds = req.exp_seconds.unwrap_or(7 * 24 * 60 * 60);
    let exp_unix = now + exp_seconds;
    let exp_time = OffsetDateTime::from_unix_timestamp(exp_unix)
        .map_err(|e| internal!(e, "Invalid exp time"))?;
    let code = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect::<String>();
    let id = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.invite_code()
                .insert_invite_code(&code, current_user_info.user_id, exp_unix)
                .await
        })
        .await
        .map_err(|e| internal!(e, "Failed to create invitation code"))?;
    Ok(general_json_res!(
        "Invitation code created",
        CreateInviteCodeRes { id, code, exp_time }
    ))
}

#[derive(Debug, Deserialize)]
pub struct InviteCodeListQueryReq {
    #[serde(flatten)]
    pub page_query: PageQueryBinder,
}

pub async fn list_invite_codes(
    current_user_info: BasicAuthData,
    State((state, cache)): InviteCodeRouterState,
    Json(req): Json<InviteCodeListQueryReq>,
) -> ApiResult<Json<GeneralResponse<PageQueryResult<InviteCodeRaw>>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("You are not allowed to list invitation codes"));
    }
    let page = state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.invite_code()
                .list_invite_codes_page(req.page_query)
                .await
        })
        .await
        .map_err(|e| internal!(e, "Failed to fetch invitation codes"))?;
    Ok(general_json_res!("Invitation codes fetched", page))
}

#[derive(Debug, Deserialize)]
pub struct InvalidateInviteCodeReq {
    pub code: Vec<String>,
}

pub async fn revoke_invite_code(
    current_user_info: BasicAuthData,
    State((state, cache)): InviteCodeRouterState,
    Json(req): Json<InvalidateInviteCodeReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!(
            "You are not allowed to invalidate invitation codes"
        ));
    }
    let issuer = req
        .code
        .iter()
        .map(|c| (c.as_str(), current_user_info.user_id))
        .collect::<Vec<_>>();
    state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.invite_code().revoke_invite_code(&issuer).await
        })
        .await
        .map_err(|e| internal!(e, "Failed to invalidate invitation codes"))?;
    Ok(general_json_res!("Invitation code invalidated"))
}
