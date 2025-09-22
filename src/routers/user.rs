use crate::get_batch_tuple;
use crate::layers::session::SessionHelper;
use crate::models::api::prelude::*;
use crate::models::dyn_setting::{AllowRegister, RegisterNeedInvitationCode};
use crate::models::session::BasicAuthData;
use crate::models::users::{Role, User, UserInternal, UserRowOptional};
use crate::services::hybrid_cache::HybridCacheService;
use crate::services::states::EchoState;
use crate::services::states::db::{DataBaseError, EchoDatabaseExecutor};
use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type UserRouterState = State<(Arc<EchoState>, Arc<HybridCacheService>)>;

#[derive(Debug, Deserialize)]
pub struct UserRegisterReq {
    pub username: String,
    pub password_hash: String,
    pub invitation_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UserRegisterRes {
    user_id: i64,
}

pub async fn user_register(
    State((state, cache)): UserRouterState,
    Json(req): Json<UserRegisterReq>,
) -> ApiResult<Json<GeneralResponse<UserRegisterRes>>> {
    let (allow_reg, reg_need_invite) = get_batch_tuple!(
        cache.dyn_settings,
        AllowRegister,
        RegisterNeedInvitationCode
    )
    .map_err(|e| internal!(e, "Failed to get dynamic settings"))?;
    let registered_user_id = state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            let user_count = exec
                .users()
                .get_user_count()
                .await
                .map_err(|e| internal!(e, "Failed to get user count"))?;
            let (permission, brand_new_server) = match user_count {
                0 => (Role::Admin, true),
                _ => (Role::User, false),
            };
            if !(allow_reg || brand_new_server) {
                return Err(bad_request!("User registration is not allowed"));
            }
            match (reg_need_invite, &req.invitation_code, brand_new_server) {
                (true, Some(code), false) => {
                    let code = exec
                        .invite_code()
                        .get_invite_code_by_code(&code)
                        .await
                        .map_err(|e| internal!(e, "Failed to query invitation code from database"))?
                        .ok_or(bad_request!("Cannot find this invitation code"))?;
                    if !code.is_valid() {
                        return Err(bad_request!("This invitation code is not valid"));
                    }
                }
                (true, None, false) => {
                    return Err(bad_request!("Invitation code is required for registration"));
                }
                _ => {}
            }
            let registered_user_id = exec
                .users()
                .add_user(&req.username, &req.password_hash, permission)
                .await
                .map_err(|e| match e {
                    DataBaseError::UniqueViolation { .. } => conflict!("Username already exists!"),
                    _ => internal!(e, "Failed to register user"),
                })?;
            if let Some(code) = req.invitation_code
                && reg_need_invite
                && !brand_new_server
            {
                exec.invite_code()
                    .revoke_invite_code(&[(code, registered_user_id)])
                    .await
                    .map_err(|e| internal!(e, "Failed to use invitation code!"))?;
            }
            Ok(registered_user_id)
        })
        .await?;
    Ok(general_json_res!(
        "User registered successfully",
        UserRegisterRes {
            user_id: registered_user_id,
        }
    ))
}

#[derive(Debug, Deserialize)]
pub struct UserLoginReq {
    pub username: String,
    pub password_hash: String,
}

#[derive(Debug, Serialize)]
pub struct UserLoginRes {
    pub need_mfa: bool,
}

pub async fn user_login(
    session: SessionHelper,
    State((state, _)): UserRouterState,
    Json(req): Json<UserLoginReq>,
) -> ApiResult<Json<GeneralResponse<UserLoginRes>>> {
    // TODO: RustRover cannot infer the type here, so fxxk u jetbrains!
    let (user, need_mfa): (UserInternal, bool) = state
        .db
        .transaction(async |mut exec: EchoDatabaseExecutor<'_>| {
            let user_row = exec
                .users()
                .query_user_by_username(&req.username)
                .await
                .map_err(|e| internal!(e, "Failed to query user from database"))?
                .ok_or(bad_request!("User not found"))?;
            let user_permission = exec
                .permission()
                .combined_query_user_permission(user_row.id, &user_row.role)
                .await?;
            let user = UserInternal {
                inner: user_row,
                permissions: user_permission,
            };
            if user.inner.password_hash != req.password_hash {
                return Err(unauthorized!("Incorrect password"));
            }
            let need_mfa = exec
                .mfa()
                .mfa_enabled(user.inner.id)
                .await
                .map_err(|e| internal!(e, "Failed to check if MFA is enabled for user"))?;
            Ok((user, need_mfa))
        })
        .await?;
    session
        .sign_basic_and_csrf_auth(user.inner.id)
        .map_err(|e| {
            internal!(
                e,
                "Failed to sign basic auth session after user registration"
            )
        })?;
    if need_mfa {
        session.sign_pre_mfa().map_err(|e| {
            internal!(
                e,
                "Failed to sign pre-MFA auth session after user registration"
            )
        })?;
    }
    Ok(general_json_res!(
        "User logged in successfully",
        UserLoginRes { need_mfa }
    ))
}

#[derive(Debug, Deserialize)]
pub struct FetchUserInfoQuery {
    pub user_id: i64,
}

pub async fn fetch_user_info(
    current_user_info: BasicAuthData,
    Query(q): Query<FetchUserInfoQuery>,
    State((_, cache)): UserRouterState,
) -> ApiResult<Json<GeneralResponse<Arc<User>>>> {
    let user = cache
        .users
        .get_user_by_user_id(q.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if user.role != Role::Admin && q.user_id != current_user_info.user_id {
        return Err(bad_request!(
            "You are not allowed to fetch other users' info"
        ));
    }
    Ok(general_json_res!("User info fetched successfully", user))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModifyUserInfoReqInner {
    pub username: Option<String>,
    pub password_hash: Option<String>,
    pub role: Option<Role>,
    pub avatar_res_id: Option<Option<i64>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModifyUserInfoReq {
    pub user_id: i64,
    pub inner: ModifyUserInfoReqInner,
}

pub async fn modify_user_info(
    current_user_info: BasicAuthData,
    State((_, cache)): UserRouterState,
    Json(req): Json<ModifyUserInfoReq>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin && req.user_id != current_user_info.user_id {
        return Err(bad_request!(
            "You are not allowed to modify other users' info"
        ));
    }
    if current_user.role != Role::Admin
        && let Some(req_role) = &req.inner.role
        && *req_role != current_user.role
    {
        return Err(bad_request!("You are not allowed to change your own role"));
    }
    let upd_row = UserRowOptional {
        id: req.user_id,
        username: req.inner.username,
        password_hash: req.inner.password_hash,
        role: req.inner.role,
        avatar_res_id: req.inner.avatar_res_id,
    };
    cache
        .users
        .update_user(upd_row)
        .await
        .map_err(|e| internal!(e, "Failed to update user info"))?;
    Ok(general_json_res!("User info updated successfully"))
}

#[derive(Debug, Deserialize)]
pub struct DeleteUserQuery {
    pub user_id: i64,
}

pub async fn delete_user(
    current_user_info: BasicAuthData,
    State((_, cache)): UserRouterState,
    Query(req): Query<DeleteUserQuery>,
) -> ApiResult<Json<GeneralResponse<()>>> {
    let current_user = cache
        .users
        .get_user_by_user_id(current_user_info.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to fetch user"))?;
    if current_user.role != Role::Admin && req.user_id != current_user_info.user_id {
        return Err(bad_request!("You are not allowed to delete other users!"));
    }
    if req.user_id == current_user_info.user_id {
        return Err(bad_request!("You are not allowed to delete yourself!"));
    }
    cache
        .users
        .remove_user_by_id(req.user_id)
        .await
        .map_err(|e| internal!(e, "Failed to delete user"))?;
    Ok(general_json_res!("User deleted successfully"))
}
