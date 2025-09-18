use crate::layers::session::{SessionError, SessionHelper};
use crate::models::api::prelude::*;
use crate::models::session::BasicAuthData;
use axum::extract::{FromRequestParts, Request as AxumExtractRequest};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::IntoResponse;

impl From<SessionError> for ApiError {
    fn from(e: SessionError) -> Self {
        match e {
            SessionError::MissingCookie(inner) => unauthorized!(err = inner),
            SessionError::MissingSessionId
            | SessionError::SessionExpired
            | SessionError::InvalidSessionId(_) => unauthorized!(err = e),
            _ => internal!(err = e),
        }
    }
}

impl<S> FromRequestParts<S> for BasicAuthData
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> ApiResult<Self> {
        parts
            .extensions
            .get::<BasicAuthData>()
            .cloned()
            .ok_or(internal!(
                "Cannot extract authed user info. Is `basic_auth_checker` enabled?"
            ))
    }
}

pub async fn basic_auth_checker(
    session: SessionHelper,
    mut request: AxumExtractRequest,
    next: Next,
) -> ApiResult<impl IntoResponse> {
    if let (_, Err(e)) = (request.method(), session.extract_csrf_auth()) {
        return Err(e.into());
    }
    let auth = session.extract_basic_auth()?;
    request.extensions_mut().insert(auth.inner);
    Ok(next.run(request).await)
}

pub async fn pre_mfa_auth_checker(
    session: SessionHelper,
    request: AxumExtractRequest,
    next: Next,
) -> ApiResult<impl IntoResponse> {
    let auth = session.extract_pre_mfa_auth()?;
    if let (true, false) = (auth.inner.need_login_mfa, auth.inner.passed_login_mfa) {
        return Err(unauthorized!("MFA required"));
    }
    Ok(next.run(request).await)
}

pub async fn mfa_auth_checker(
    session: SessionHelper,
    request: AxumExtractRequest,
    next: Next,
) -> ApiResult<impl IntoResponse> {
    session.extract_mfa_auth()?;
    Ok(next.run(request).await)
}
