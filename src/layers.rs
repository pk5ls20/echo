pub mod auth;
pub mod client_info;
pub mod session;

// TODO: Selective use of `[ClientInfoLayer]`
#[macro_export]
macro_rules! echo_layer_builder {
    ($state:expr $(,)?) => {
        || {
            tower::ServiceBuilder::new()
                .layer($crate::layers::session::SessionLayer::new($state.clone()))
                .layer($crate::layers::client_info::ClientInfoLayer::new())
        }
    };
    ($state:expr,b $(,)?) => {
        || {
            tower::ServiceBuilder::new()
                .layer($crate::layers::session::SessionLayer::new($state.clone()))
                .layer($crate::layers::client_info::ClientInfoLayer::new())
                .layer(axum::middleware::from_fn(
                    $crate::layers::auth::basic_auth_checker,
                ))
        }
    };
    ($state:expr,b,pm $(,)?) => {
        || {
            tower::ServiceBuilder::new()
                .layer($crate::layers::session::SessionLayer::new($state.clone()))
                .layer($crate::layers::client_info::ClientInfoLayer::new())
                .layer(axum::middleware::from_fn(
                    $crate::layers::auth::basic_auth_checker,
                ))
                .layer(axum::middleware::from_fn(
                    $crate::layers::auth::pre_mfa_auth_checker,
                ))
        }
    };
    ($state:expr,b,pm,m $(,)?) => {
        || {
            tower::ServiceBuilder::new()
                .layer($crate::layers::session::SessionLayer::new($state.clone()))
                .layer($crate::layers::client_info::ClientInfoLayer::new())
                .layer(axum::middleware::from_fn(
                    $crate::layers::auth::basic_auth_checker,
                ))
                .layer(axum::middleware::from_fn(
                    $crate::layers::auth::pre_mfa_auth_checker,
                ))
                .layer(axum::middleware::from_fn(
                    $crate::layers::auth::mfa_auth_checker,
                ))
        }
    };
}
