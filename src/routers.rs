use crate::echo_layer_builder;
use crate::routers::echo::{add_echo, delete_echo, list_echo, modify_echo};
use crate::routers::invite_code::{create_invite_code, list_invite_codes, revoke_invite_code};
use crate::routers::mfa::{
    get_mfa_infos, get_mfa_op_logs, totp_delete, totp_list, totp_setup_finish, totp_setup_start,
    totp_verify, webauthn_auth_finish, webauthn_auth_start, webauthn_delete, webauthn_list,
    webauthn_setup_finish, webauthn_setup_start,
};
use crate::routers::permission::{
    add_permission, delete_permission, get_permission_info, get_permission_records,
    grant_permission, modify_permission, revoke_permission,
};
use crate::routers::resource::{
    delete_resource, get_resource_by_ids, update_resource, upload_chunk, upload_commit,
    upload_create,
};
use crate::routers::settings::{get_dyn_settings, get_static_settings, set_dyn_settings};
use crate::routers::user::{
    delete_user, fetch_user_info, modify_user_info, user_login, user_register,
};
use crate::services::echo_baker::EchoBaker;
use crate::services::hybrid_cache::HybridCacheService;
use crate::services::mfa::MFAService;
use crate::services::states::EchoState;
use crate::services::upload_tracker::UploadTrackerService;
use axum::Router;
use axum::http::{HeaderName, Request};
use axum::routing::{get, patch, post, put};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::request_id::{
    MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer,
};
use tower_http::trace::TraceLayer;
use tracing::info_span;

mod echo;
mod invite_code;
mod mfa;
mod permission;
mod resource;
mod settings;
mod user;

pub async fn router(state: Arc<EchoState>) -> Router {
    // TODO: When more services are added in the future, maybe we can write a `ServiceBuilder`?.
    let mfa_service = {
        Arc::new(
            MFAService::new(state.clone())
                .await
                .expect("Failed to init MFAService"),
        )
    };
    let upload_tracker_service = {
        Arc::new(
            UploadTrackerService::new(state.clone())
                .await
                .expect("Failed to init UploadTrackerService"),
        )
    };
    let hybrid_cache_service = Arc::new(HybridCacheService::new(state.clone()));
    let echo_baker_service = Arc::new(EchoBaker::new());
    let raw_layer = echo_layer_builder!(state);
    let basic_layer = echo_layer_builder!(state, b);
    let full_mfa_layer = echo_layer_builder!(state, b, m);
    let user_router = {
        Router::new()
            .merge(
                Router::new()
                    .route("/register", post(user_register))
                    .route("/login", post(user_login))
                    .layer(raw_layer()),
            )
            .merge(
                Router::new()
                    .route("/info", get(fetch_user_info))
                    .layer(basic_layer()),
            )
            .merge(
                Router::new()
                    .route("/info", patch(modify_user_info).delete(delete_user))
                    .layer(full_mfa_layer()),
            )
            .with_state((state.clone(), hybrid_cache_service.clone()))
    };
    let mfa_router = {
        Router::new()
            .merge(
                Router::new()
                    .route("/", post(get_mfa_infos))
                    .layer(basic_layer())
                    .merge(
                        Router::new()
                            .route("/logs", post(get_mfa_op_logs))
                            .layer(full_mfa_layer()),
                    ),
            )
            .nest(
                "/totp",
                Router::new()
                    .route("/setup/start", post(totp_setup_start))
                    .route("/setup/finish", post(totp_setup_finish))
                    .route("/verify", post(totp_verify))
                    .layer(basic_layer())
                    .merge(
                        Router::new()
                            .route("/", get(totp_list).delete(totp_delete))
                            .layer(full_mfa_layer()),
                    ),
            )
            .nest(
                "/webauthn",
                Router::new()
                    .route("/setup/start", post(webauthn_setup_start))
                    .route("/setup/finish", post(webauthn_setup_finish))
                    .route("/auth/start", post(webauthn_auth_start))
                    .route("/auth/finish", post(webauthn_auth_finish))
                    .layer(basic_layer())
                    .merge(
                        Router::new()
                            .route("/", get(webauthn_list).delete(webauthn_delete))
                            .layer(full_mfa_layer()),
                    ),
            )
            .with_state((state.clone(), mfa_service, hybrid_cache_service.clone()))
    };
    let resource_router = {
        Router::new()
            .nest(
                "/upload",
                Router::new()
                    .route("/create", post(upload_create))
                    .route("/chunk", put(upload_chunk))
                    .route("/commit", post(upload_commit)),
            )
            .route(
                "/",
                post(get_resource_by_ids)
                    .patch(update_resource)
                    .delete(delete_resource),
            )
            .layer(full_mfa_layer())
            .with_state((
                state.clone(),
                upload_tracker_service,
                hybrid_cache_service.clone(),
            ))
    };
    let invite_code_router = {
        Router::new()
            .route(
                "/",
                post(list_invite_codes)
                    .put(create_invite_code)
                    .delete(revoke_invite_code),
            )
            .layer(full_mfa_layer())
            .with_state((state.clone(), hybrid_cache_service.clone()))
    };
    let permission_router = {
        Router::new()
            .route(
                "/item",
                put(add_permission)
                    .patch(modify_permission)
                    .delete(delete_permission),
            )
            .route(
                "/",
                get(get_permission_info)
                    .put(grant_permission)
                    .delete(revoke_permission),
            )
            .route("/records", post(get_permission_records))
            .layer(full_mfa_layer())
    };
    let echo_router = {
        Router::new()
            .route(
                "/",
                put(add_echo)
                    .patch(modify_echo)
                    .delete(delete_echo)
                    .post(list_echo),
            )
            .layer(full_mfa_layer())
            .with_state((
                state.clone(),
                hybrid_cache_service.clone(),
                echo_baker_service,
            ))
    };
    let settings_router = {
        Router::new()
            .route("/dynamic", post(get_dyn_settings).patch(set_dyn_settings))
            .route("/static", post(get_static_settings))
            .layer(full_mfa_layer())
            .with_state((state.clone(), hybrid_cache_service.clone()))
    };
    let trace_header = HeaderName::from_static("x-hananokioku");
    Router::new()
        .nest(
            "/api/v1",
            Router::new()
                .nest("/user", user_router)
                .nest("/mfa", mfa_router)
                .nest("/resource", resource_router)
                .nest("/invite-code", invite_code_router)
                .nest("/permission", permission_router)
                .nest("/echo", echo_router)
                .nest("/settings", settings_router),
        )
        .layer(
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::new(
                    trace_header.clone(),
                    MakeRequestUuid,
                ))
                .layer(
                    TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
                        let rid = req
                            .extensions()
                            .get::<RequestId>()
                            .and_then(|r| r.header_value().to_str().ok())
                            .expect("Cannot get request id");
                        info_span!(
                            "http.request",
                            request_id = %rid,
                            method = %req.method(),
                            uri = %req.uri(),
                            version = ?req.version(),
                        )
                    }),
                )
                .layer(PropagateRequestIdLayer::new(trace_header))
                .concurrency_limit(state.config.common.concurrency_limit),
        )
        .with_state((state, hybrid_cache_service))
}
