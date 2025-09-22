use crate::layers::client_info::ClientInfo;
use crate::layers::session::SessionHelper;
use crate::models::api::prelude::*;
use crate::models::mfa::{
    MFAAuthMethod, MFAOpType, MfaAuthLog, MfaInfo, NewMfaAuthLog, NewMfaAuthLogInfo,
    WebauthnCredential,
};
use crate::models::session::BasicAuthData;
use crate::models::users::Role;
use crate::services::hybrid_cache::HybridCacheService;
use crate::services::mfa::MFAService;
use crate::services::states::EchoState;
use crate::services::states::db::{
    DataBaseError, EchoDatabaseExecutor, PageQueryBinder, PageQueryResult,
};
use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use time::OffsetDateTime;
use totp_rs::TOTP;
use webauthn_rs::prelude::*;

pub type MFARouterState = State<(Arc<EchoState>, Arc<MFAService>, Arc<HybridCacheService>)>;

#[derive(Debug, Serialize)]
pub struct TotpSetupRes {
    totp_uri: String,
}

pub async fn totp_setup(
    client_info: ClientInfo,
    current_user_info: BasicAuthData,
    State((state, mfa_service, cache)): MFARouterState,
) -> ApiResult<Json<GeneralResponse<TotpSetupRes>>> {
    let current_user_id = current_user_info.user_id;
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    // TODO: RustRover cannot infer the type here, so fxxk u jetbrains!
    let totp: TOTP = state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            if let Some(info) = exec
                .mfa()
                .list_user_totp_credential(current_user_id)
                .await
                .map_err(|e| internal!(e, "Failed to get MFA info"))?
            {
                return Err(conflict!(format!(
                    "TOTP already configured at {}, last used at {:?}",
                    info.created_at, info.last_used_at
                )));
            }
            let totp = mfa_service
                .generate_totp(current_user.username.clone())
                .map_err(|e| internal!(e, "Failed to generate TOTP"))?;
            let cred = mfa_service
                .save_totp(current_user_id, &totp)
                .map_err(|e| internal!(e, "Failed to serialize TOTP"))?;
            exec.mfa()
                .insert_totp_credential(cred, client_info.ip_address, client_info.user_agent)
                .await
                .map_err(|e| internal!(e, "Failed to save TOTP credential"))?;
            exec.mfa()
                .enable_mfa(current_user_info.user_id)
                .await
                .map_err(|e| internal!(e, "Failed to enable mfa"))?;
            Ok(totp)
        })
        .await?;
    Ok(general_json_res!(
        "TOTP created",
        TotpSetupRes {
            totp_uri: totp.get_url(),
        }
    ))
}

#[derive(Debug, Deserialize)]
pub struct TotpVerifyReq {
    code: String,
}

pub async fn totp_verify(
    session: SessionHelper,
    current_user_info: BasicAuthData,
    State((state, mfa_service, _)): MFARouterState,
    client_info: ClientInfo,
    Json(req): Json<TotpVerifyReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let user_id = current_user_info.user_id;
    state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            let cred = exec
                .mfa()
                .list_user_totp_credential(user_id)
                .await
                .map_err(|e| internal!(e, "Failed to load TOTP credential"))?
                .ok_or(bad_request!("TOTP is not configured"))?;
            let totp = mfa_service
                .load_totp(&cred.totp_credential_data)
                .map_err(|e| internal!(e, "Failed to decode TOTP"))?;
            let current = totp
                .generate_current()
                .map_err(|e| internal!(e, "Failed to generate current TOTP"))?;
            if current != req.code {
                exec.mfa()
                    .insert_mfa_op_access_log(
                        user_id,
                        NewMfaAuthLog {
                            user_id,
                            op_type: MFAOpType::Auth,
                            info: NewMfaAuthLogInfo {
                                auth_method: MFAAuthMethod::Totp,
                                is_success: false,
                                ip_address: client_info.ip_address,
                                user_agent: client_info.user_agent,
                                credential_id: Some(cred.id),
                                error_message: Some("Invalid TOTP code".to_string()),
                            },
                        },
                    )
                    .await
                    .map_err(|e| internal!(e, "Failed to insert MFA log"))?;
                return Err(bad_request!("Invalid TOTP code"));
            }
            exec.mfa()
                .update_totp_last_used(user_id)
                .await
                .map_err(|e| internal!(e, "Failed to update last used"))?;
            exec.mfa()
                .insert_mfa_op_access_log(
                    user_id,
                    NewMfaAuthLog {
                        user_id,
                        op_type: MFAOpType::Auth,
                        info: NewMfaAuthLogInfo {
                            auth_method: MFAAuthMethod::Totp,
                            is_success: true,
                            ip_address: client_info.ip_address,
                            user_agent: client_info.user_agent,
                            credential_id: Some(cred.id),
                            error_message: None,
                        },
                    },
                )
                .await
                .map_err(|e| internal!(e, "Failed to insert MFA log"))?;
            Ok(())
        })
        .await?;
    session
        .sign_mfa()
        .map_err(|e| internal!(e, "Failed to sign MFA session"))?;
    Ok(general_json_res!("MFA verified"))
}

#[derive(Debug, Serialize)]
pub struct TotpListItem {
    id: i64,
    #[serde(with = "time::serde::timestamp")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp")]
    updated_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp::option")]
    last_used_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
pub struct TotpListRes {
    list: Vec<TotpListItem>,
}

#[derive(Debug, Deserialize)]
pub struct TotpListQueryReq {
    user_id: i64,
}

pub async fn totp_list(
    current_user_info: BasicAuthData,
    State((state, _, cache)): MFARouterState,
    Query(q): Query<TotpListQueryReq>,
) -> ApiResult<Json<GeneralResponse<TotpListRes>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if q.user_id != current_user_info.user_id && current_user.role != Role::Admin {
        return Err(bad_request!("Cannot list TOTP for other users"));
    }
    // TODO: RustRover cannot infer the type here, so fxxk u jetbrains!
    let list: Vec<TotpListItem> = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.mfa().list_user_totp_credential(q.user_id).await
        })
        .await
        .map_err(|e| internal!(e, "Failed to list TOTP"))?
        .map(|c| TotpListItem {
            id: c.id,
            created_at: c.created_at,
            updated_at: c.updated_at,
            last_used_at: c.last_used_at,
        })
        .into_iter()
        .collect();
    Ok(general_json_res!("OK", TotpListRes { list }))
}

#[derive(Debug, Deserialize)]
pub struct DeleteTotpReq {
    user_id: i64,
}

pub async fn totp_delete(
    current_user_info: BasicAuthData,
    client_info: ClientInfo,
    State((state, _, cache)): MFARouterState,
    Json(req): Json<DeleteTotpReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if req.user_id != current_user_info.user_id && current_user.role != Role::Admin {
        return Err(bad_request!("Cannot list TOTP for other users"));
    }
    state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.mfa()
                .delete_totp_credential(req.user_id, client_info.ip_address, client_info.user_agent)
                .await?;
            Ok::<_, DataBaseError>(())
        })
        .await
        .map_err(|e| internal!(e, "Failed to delete TOTP"))?;
    Ok(general_json_res!("TOTP deleted"))
}

pub async fn webauthn_setup_start(
    current_user_info: BasicAuthData,
    State((state, mfa_service, cache)): MFARouterState,
) -> ApiResult<Json<GeneralResponse<CreationChallengeResponse>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    let existing = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.mfa()
                .list_user_webauthn_credentials(current_user_info.user_id)
                .await
        })
        .await
        .map_err(|e| internal!(e, "Failed to list existing passkeys"))?;
    let ccr = mfa_service
        .start_passkey_registration(
            current_user_info.user_id,
            &current_user.username,
            (!existing.is_empty()).then_some(existing),
        )
        .await
        .map_err(|e| internal!(e, "Failed to start passkey registration"))?;
    Ok(general_json_res!("OK", ccr))
}

pub async fn webauthn_setup_finish(
    current_user_info: BasicAuthData,
    client_info: ClientInfo,
    State((state, mfa_service, cache)): MFARouterState,
    Json(req): Json<RegisterPublicKeyCredential>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    let cred = mfa_service
        .finish_passkey_registration(
            current_user_info.user_id,
            current_user.username.clone(),
            req,
        )
        .await
        .map_err(|e| internal!(e, "Failed to finish passkey registration"))?;
    state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.mfa()
                .insert_webauthn_credential(cred, client_info.ip_address, client_info.user_agent)
                .await
                .map_err(|e| internal!(e, "Failed to insert passkey"))?;
            exec.mfa()
                .enable_mfa(current_user_info.user_id)
                .await
                .map_err(|e| internal!(e, "Failed to enable mfa"))?;
            Ok::<_, ApiError>(())
        })
        .await?;
    Ok(general_json_res!("Passkey registered"))
}

pub async fn webauthn_auth_start(
    current_user_info: BasicAuthData,
    State((state, mfa_service, cache)): MFARouterState,
) -> ApiResult<Json<GeneralResponse<RequestChallengeResponse>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    // TODO: RustRover cannot infer the type here, so fxxk u jetbrains!
    let existing: Vec<WebauthnCredential> = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.mfa()
                .list_user_webauthn_credentials(current_user_info.user_id)
                .await
        })
        .await
        .map_err(|e| internal!(e, "Failed to list existing passkeys"))?;
    if existing.is_empty() {
        return Err(bad_request!("No passkey configured"));
    }
    let rcr = mfa_service
        .start_passkey_authentication(current_user.username.clone(), existing)
        .await
        .map_err(|e| internal!(e, "Failed to start passkey authentication"))?;
    Ok(general_json_res!("OK", rcr))
}

pub async fn webauthn_auth_finish(
    session: SessionHelper,
    current_user_info: BasicAuthData,
    State((state, mfa_service, cache)): MFARouterState,
    client_info: ClientInfo,
    Json(req): Json<PublicKeyCredential>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            match mfa_service
                .finish_passkey_authentication(current_user.username.clone(), req)
                .await
            {
                Ok(_) => {
                    exec.mfa()
                        .insert_mfa_op_access_log(
                            current_user_info.user_id,
                            NewMfaAuthLog {
                                user_id: current_user_info.user_id,
                                op_type: MFAOpType::Auth,
                                info: NewMfaAuthLogInfo {
                                    auth_method: MFAAuthMethod::Webauthn,
                                    is_success: true,
                                    ip_address: client_info.ip_address,
                                    user_agent: client_info.user_agent,
                                    credential_id: None,
                                    error_message: None,
                                },
                            },
                        )
                        .await
                        .map_err(|e| internal!(e, "Failed to insert MFA log"))?;
                }
                Err(e) => {
                    exec.mfa()
                        .insert_mfa_op_access_log(
                            current_user_info.user_id,
                            NewMfaAuthLog {
                                user_id: current_user_info.user_id,
                                op_type: MFAOpType::Auth,
                                info: NewMfaAuthLogInfo {
                                    auth_method: MFAAuthMethod::Webauthn,
                                    is_success: false,
                                    ip_address: client_info.ip_address,
                                    user_agent: client_info.user_agent,
                                    credential_id: None,
                                    error_message: Some(format!("{}", e)),
                                },
                            },
                        )
                        .await
                        .map_err(|e| internal!(e, "Failed to insert MFA log"))?;
                    return Err(bad_request!(e, "Failed to finish passkey authentication"));
                }
            }
            Ok(())
        })
        .await?;
    session
        .sign_mfa()
        .map_err(|e| internal!(e, "Failed to sign MFA session"))?;
    Ok(general_json_res!("MFA verified"))
}

#[derive(Debug, Serialize)]
pub struct WebauthnListRes {
    list: Vec<WebauthnCredentialInfo>,
}

#[derive(Debug, Serialize)]
pub struct WebauthnCredentialInfo {
    id: i64,
    user_display_name: Option<String>,
    #[serde(with = "time::serde::timestamp")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp")]
    updated_at: OffsetDateTime,
    #[serde(with = "time::serde::timestamp::option")]
    last_used_at: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize)]
pub struct WebauthnListQuery {
    user_id: i64,
}

pub async fn webauthn_list(
    current_user_info: BasicAuthData,
    State((state, _, cache)): MFARouterState,
    Query(q): Query<WebauthnListQuery>,
) -> ApiResult<Json<GeneralResponse<WebauthnListRes>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if q.user_id != current_user_info.user_id && current_user.role != Role::Admin {
        return Err(bad_request!("Cannot list TOTP for other users"));
    }
    // TODO: RustRover cannot infer the type here, so fxxk u jetbrains!
    let list: Vec<WebauthnCredentialInfo> = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.mfa().list_user_webauthn_credentials(q.user_id).await
        })
        .await
        .map_err(|e| internal!(e, "Failed to list passkeys"))?
        .into_iter()
        .map(|c| WebauthnCredentialInfo {
            id: c.id,
            user_display_name: c.user_display_name,
            created_at: c.created_at,
            updated_at: c.updated_at,
            last_used_at: c.last_used_at,
        })
        .collect();
    Ok(general_json_res!("OK", WebauthnListRes { list }))
}

#[derive(Debug, Deserialize)]
pub struct DeleteWebauthnQuery {
    credential_id: i64,
}

pub async fn webauthn_delete(
    current_user_info: BasicAuthData,
    State((state, _, cache)): MFARouterState,
    client_info: ClientInfo,
    Query(q): Query<DeleteWebauthnQuery>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            let cred = exec
                .mfa()
                .get_webauthn_credential_by_id(q.credential_id)
                .await
                .map_err(|e| internal!(e, "Failed to query passkey"))?;
            if let Some(cred) = cred {
                if cred.user_id != current_user_info.user_id && current_user.role != Role::Admin {
                    return Err(bad_request!("Cannot delete others' passkey"));
                }
                exec.mfa()
                    .delete_webauthn_credential(
                        q.credential_id,
                        client_info.ip_address,
                        client_info.user_agent,
                    )
                    .await
                    .map_err(|e| internal!(e, "Failed to delete passkey"))?;
            }
            Ok(())
        })
        .await?;
    Ok(general_json_res!("Passkey deleted"))
}

#[derive(Debug, Deserialize)]
pub struct GetMfaInfoReq {
    user_ids: Vec<i64>,
}

pub async fn get_mfa_infos(
    current_user_info: BasicAuthData,
    State((state, _, cache)): MFARouterState,
    Json(req): Json<GetMfaInfoReq>,
) -> ApiResult<Json<GeneralResponse<Vec<MfaInfo>>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin && req.user_ids.as_slice() != [current_user_info.user_id] {
        return Err(bad_request!("Cannot get MFA info for other users"));
    }
    let res = state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            if let Some(id) = exec
                .users()
                .check_user_exists(&req.user_ids)
                .await
                .map_err(|e| internal!(e, "Failed to check user exists"))?
            {
                return Err(bad_request!(format!("User ID {} not exists!", id)));
            }
            exec.mfa()
                .get_mfa_infos(&req.user_ids)
                .await
                .map_err(|e| internal!(e, "Failed to get MFA info"))
        })
        .await?;
    Ok(general_json_res!("OK", res))
}

#[derive(Debug, Deserialize)]
pub struct MfaLogsQueryReq {
    #[serde(flatten)]
    page_query: PageQueryBinder,
}

pub async fn get_mfa_op_logs(
    current_user_info: BasicAuthData,
    State((state, _, cache)): MFARouterState,
    Json(req): Json<MfaLogsQueryReq>,
) -> ApiResult<Json<GeneralResponse<PageQueryResult<MfaAuthLog>>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin {
        return Err(bad_request!("Only admin can view MFA operation logs"));
    }
    let list = state
        .db
        .single(async |mut exec: EchoDatabaseExecutor<'_>| {
            exec.mfa()
                .get_mfa_op_logs_page(current_user_info.user_id, req.page_query)
                .await
        })
        .await
        .map_err(|e| internal!(&e, "Failed to get MFA operation logs!"))?;
    Ok(general_json_res!("OK", list))
}
