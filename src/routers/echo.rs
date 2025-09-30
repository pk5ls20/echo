use crate::gladiator::ext_plugins::EchoExtMetaPubInfo;
use crate::models::api::prelude::*;
use crate::models::echo::Echo;
use crate::models::session::BasicAuthData;
use crate::models::users::Role;
use crate::services::echo_baker::{EchoBaker, EchoBakerError};
use crate::services::hybrid_cache::HybridCacheService;
use crate::services::states::EchoState;
use crate::services::states::db::{EchoDatabaseExecutor, PageQueryBinder, PageQueryResult};
use ahash::HashMap;
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type EchoRouterState = State<(
    Arc<EchoState>,
    Arc<HybridCacheService>,
    Arc<EchoBaker<'static>>,
)>;

#[derive(Debug, Serialize, Deserialize)]
pub struct EchoInfo {
    content: String,
    echo_permission_ids: Vec<i64>,
    is_private: bool,
}

#[derive(Debug, Deserialize)]
pub struct AddEchoReq {
    #[serde(flatten)]
    inner: EchoInfo,
}

pub async fn add_echo(
    current_user_info: BasicAuthData,
    State((state, cache, baker)): EchoRouterState,
    Json(req): Json<AddEchoReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    // TODO: Allow users to have their own ext
    let baked = baker
        .add_outer_echo(
            &req.inner.content,
            &current_user.permission_ids,
            EchoBaker::all_ext_ids(),
        )
        .map_err(|e| internal!(e, "Failed to add echo"))?;
    state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.echo()
                .add_echo(
                    current_user_info.user_id,
                    &baked.safe_echo,
                    baked.res_ids.as_deref().unwrap_or_default(),
                    &req.inner.echo_permission_ids,
                    req.inner.is_private,
                )
                .await
        })
        .await
        .map_err(|e| internal!(e, "Failed to add echo"))?;
    Ok(general_json_res!("Echo added successfully"))
}

#[derive(Debug, Deserialize)]
pub struct ModifyEchoReq {
    echo_id: i64,
    #[serde(flatten)]
    inner: EchoInfo,
}

pub async fn modify_echo(
    current_user_info: BasicAuthData,
    State((state, cache, baker)): EchoRouterState,
    Json(req): Json<ModifyEchoReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    // TODO: Allow users to have their own ext
    let baked = baker
        .add_outer_echo(
            &req.inner.content,
            &current_user.permission_ids,
            EchoBaker::all_ext_ids(),
        )
        .map_err(|e| internal!(e, "Failed to add echo"))?;
    state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.echo()
                .update_echo(
                    req.echo_id,
                    &baked.safe_echo,
                    baked.res_ids.as_deref().unwrap_or_default(),
                    &req.inner.echo_permission_ids,
                    req.inner.is_private,
                )
                .await
        })
        .await
        .map_err(|e| internal!(e, "Failed to update echo"))?;
    Ok(general_json_res!("Echo updated successfully"))
}

#[derive(Debug, Deserialize)]
pub struct DeleteEchoReq {
    echo_id: i64,
}

pub async fn delete_echo(
    current_user_info: BasicAuthData,
    State((state, cache, _)): EchoRouterState,
    Json(req): Json<DeleteEchoReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    // TODO: RustRover cannot infer the type here, so fxxk u jetbrains!
    let maybe_delete_echo: Option<Echo> = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.echo().query_echo_by_id(req.echo_id).await
        })
        .await
        .map_err(|e| internal!(e, "Failed to fetch echo"))?;
    match maybe_delete_echo {
        Some(echo) => {
            if echo.has_permission(&current_user) {
                return Err(bad_request!("No permission to delete this echo"));
            }
            if echo.user_id != current_user_info.user_id && current_user.role != Role::Admin {
                return Err(bad_request!("Can only delete your own echo"));
            }
            state
                .db
                .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
                    exec.echo().delete_echo(req.echo_id).await
                })
                .await
                .map_err(|e| internal!(e, "Failed to delete echo"))?;
            Ok(general_json_res!("Echo deleted successfully"))
        }
        None => Err(bad_request!("Echo not found")),
    }
}

#[derive(Debug, Deserialize)]
pub struct ListEchoReq {
    pub user_id: Option<i64>,
    #[serde(flatten)]
    pub page_query: PageQueryBinder,
}

pub async fn list_echo(
    current_user_info: BasicAuthData,
    State((state, cache, baker)): EchoRouterState,
    Json(req): Json<ListEchoReq>,
) -> ApiResult<Json<GeneralResponse<PageQueryResult<Echo>>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    let mut echos = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.echo()
                .query_user_echo(req.user_id, req.page_query)
                .await
        })
        .await
        .map_err(|e| internal!(e, "Failed to fetch echo"))?;
    echos
        .items
        .iter_mut()
        .try_for_each(|it| {
            it.content = match it.has_permission(&current_user) {
                true => Some(baker.post_inner_echo(
                    Arc::downgrade(&state),
                    it.content.as_deref().unwrap(),
                    current_user_info.user_id,
                    &current_user.permission_ids,
                    EchoBaker::all_ext_ids(),
                )?),
                false => None,
            };
            Ok(())
        })
        .map_err(|e: EchoBakerError| internal!(e, "Failed to bake echo content"))?;
    Ok(general_json_res!("Successfully fetched echos", echos))
}

pub async fn list_echo_ext(
    State(_): EchoRouterState,
) -> ApiResult<Json<GeneralResponse<&'static HashMap<u32, EchoExtMetaPubInfo>>>> {
    Ok(general_json_res!(
        "Successfully fetched echo ext info",
        EchoBaker::all_ext_metas()
    ))
}
