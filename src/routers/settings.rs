use crate::models::api::prelude::*;
use crate::models::dyn_setting::{DynSettingCollector, DynSettingsKvMap, DynSettingsValue};
use crate::services::hybrid_cache::HybridCacheService;
use crate::services::states::EchoState;
use crate::services::states::config::AppConfig;
use crate::services::states::db::EchoDatabaseExecutor;
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::Arc;

pub type DynSettingsRouterState = State<(Arc<EchoState>, Arc<HybridCacheService>)>;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GetDynSettingsReqInner {
    All,
    Single(Cow<'static, str>),
}

#[derive(Debug, Deserialize)]
pub struct GetDynSettingsReq {
    #[serde(flatten)]
    req: GetDynSettingsReqInner,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum GetDynSettingsResInner<'a> {
    All(DynSettingsKvMap<'a>),
    Single(DynSettingsValue<'a>),
}

#[derive(Debug, Serialize)]
pub struct GetDynSettingsRes {
    result: GetDynSettingsResInner<'static>,
}

pub async fn get_dyn_settings(
    State((state, cache)): DynSettingsRouterState,
    Json(req): Json<GetDynSettingsReq>,
) -> ApiResult<Json<GeneralResponse<GetDynSettingsRes>>> {
    let res = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            let res = match req.req {
                GetDynSettingsReqInner::All => GetDynSettingsRes {
                    result: GetDynSettingsResInner::All(
                        cache
                            .dyn_settings
                            .get_all_kvs()
                            .await
                            .map_err(|e| internal!(e, "Failed to get all kvs"))?,
                    ),
                },
                GetDynSettingsReqInner::Single(key) => GetDynSettingsRes {
                    result: GetDynSettingsResInner::Single(
                        cache
                            .dyn_settings
                            .get_with_str(key, &mut exec)
                            .await
                            .map_err(|e| internal!(e, "Failed to get single kv"))?,
                    ),
                },
            };
            Ok::<_, ApiError>(res)
        })
        .await?;
    Ok(general_json_res!("Successfully got dyn settings", res))
}

#[derive(Debug, Deserialize)]
pub struct SetDynSettingsReq {
    key: String,
    new_value: String,
    overwrite: Option<bool>,
}

pub async fn set_dyn_settings(
    State((_, cache)): DynSettingsRouterState,
    Json(req): Json<SetDynSettingsReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    DynSettingCollector::try_parse(&req.key, &req.new_value)
        .ok_or(internal!("Cannot find the given key"))?
        .map_err(|e| internal!(e, "Failed to parse new value for the given key"))?;
    cache
        .dyn_settings
        .set_with_str(&req.key, &req.new_value, req.overwrite.unwrap_or_default())
        .await
        .map_err(|e| internal!(e, "Failed to set dyn setting"))?;
    Ok(general_json_res!("Successfully set dyn setting", ()))
}

#[derive(Debug, Deserialize)]
pub struct GetStaticSettingsReq {
    is_default: bool,
}

pub async fn get_static_settings(
    State((state, _)): DynSettingsRouterState,
    Json(req): Json<GetStaticSettingsReq>,
) -> ApiResult<Json<GeneralResponse<Arc<AppConfig>>>> {
    let app_config = match req.is_default {
        true => Arc::new(AppConfig::default()),
        false => state.config.clone(),
    };
    Ok(general_json_res!(
        "Successfully got static settings",
        app_config
    ))
}
