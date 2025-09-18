use crate::models::api::prelude::*;
use crate::models::session::{
    BaseSession, BasicAuthData, BasicAuthSessionData, CsrfAuthData, CsrfAuthSessionData,
    MfaAuthData, MfaAuthSessionData, PreMfaAuthData, PreMfaAuthSessionData,
};
use crate::services::states::EchoState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{Request, Response};
use base64::{Engine as _, engine::general_purpose as b64_general_engine};
use cookie::{Cookie, SameSite};
use echo_macros::EchoBusinessError;
use std::ops::Add;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::{Layer, Service};
use tower_cookies::{CookieManager, CookieManagerLayer, Cookies, PrivateCookies};
use uuid::Uuid;

#[derive(Clone)]
pub struct SessionHelper {
    state: Arc<EchoState>,
    cookies: Cookies,
}

#[derive(Debug, thiserror::Error, EchoBusinessError)]
pub enum SessionError {
    // Missing
    #[error("Cannot extract session id in cookies")]
    #[code(10000)]
    MissingSessionId,
    #[error(transparent)]
    #[code(11000)]
    MissingCookie(#[from] MissingCookieError),
    // Expired
    #[error("Your session has expired")]
    #[code(12000)]
    SessionExpired,
    // Invalid
    #[error("Invalid Session ID: {0}, please log in again")]
    #[code(13000)]
    InvalidSessionId(Uuid),
    // misc
    #[error(transparent)]
    Base64Decode(#[from] base64::DecodeError),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error(transparent)]
    MessagePackEncode(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    MessagePackDecode(#[from] rmp_serde::decode::Error),
}

pub type SessionResult<T> = Result<T, SessionError>;

impl SessionHelper {
    pub fn new(state: Arc<EchoState>, cookies: Cookies) -> Self {
        Self { state, cookies }
    }

    fn session_id(&self) -> &Uuid {
        self.state.auth.get_session_id()
    }

    fn basic_auth_jar(&self) -> PrivateCookies<'_> {
        self.cookies.private(self.state.auth.get_basic_auth_key())
    }

    fn csrf_auth_jar(&self) -> PrivateCookies<'_> {
        self.cookies.private(self.state.auth.get_csrf_auth_key())
    }

    fn pre_mfa_auth_jar(&self) -> PrivateCookies<'_> {
        self.cookies.private(self.state.auth.get_pre_mfa_auth_key())
    }

    fn mfa_auth_jar(&self) -> PrivateCookies<'_> {
        self.cookies.private(self.state.auth.get_mfa_auth_key())
    }

    pub fn sign_basic_and_csrf_auth(&self, user_id: i64) -> SessionResult<()> {
        self.sign_basic_auth(BasicAuthData::new(user_id))?;
        self.sign_csrf_auth(())?;
        Ok(())
    }

    pub fn sign_pre_mfa(&self, need_login_mfa: bool, passed_login_mfa: bool) -> SessionResult<()> {
        self.sign_pre_mfa_auth(PreMfaAuthData::new(need_login_mfa, passed_login_mfa))
    }

    pub fn sign_mfa(&self) -> SessionResult<()> {
        self.sign_mfa_auth(())
    }
}

macro_rules! auth_session {
    (
        $( $base_name:ident => ( data = $inner_ty:ty, same_site = $same_site:expr, biz_code = $biz_code:expr ) ),* $(,)?
    ) => {
        paste::paste! {
            #[derive(Debug, thiserror::Error, EchoBusinessError)]
            pub enum MissingCookieError {
                $(
                    #[error("Failed to extract {cookie} cookie", cookie = stringify!($base_name))]
                    #[code($biz_code)]
                    [<$base_name:camel>],
                )*
            }
        }
        $(
            paste::paste! {
                impl SessionHelper {
                    fn [<sign_ $base_name>](&self, inner: $inner_ty) -> SessionResult<()> {
                        use crate::models::const_val::[<ECHO_ $base_name:snake:upper>];
                        use crate::models::const_val::[<ECHO_ $base_name:snake:upper _EXPIRE>];
                        let sess: [<$base_name:camel SessionData>] = BaseSession::new(*self.session_id(), inner);
                        let bytes = rmp_serde::to_vec(&sess)?;
                        let enc = b64_general_engine::URL_SAFE.encode(bytes);
                        self.[<$base_name _jar>]().add(
                            Cookie::build(([<ECHO_ $base_name:snake:upper>], enc))
                                .path("/")
                                .secure(cfg!(feature = "secure-cookie"))
                                .http_only(cfg!(feature = "secure-cookie"))
                                .max_age([<ECHO_ $base_name:snake:upper _EXPIRE>])
                                .same_site($same_site)
                                .build(),
                        );
                        Ok(())
                    }
                    pub(crate) fn [<extract_ $base_name>](&self) -> SessionResult<[<$base_name:camel SessionData>]> {
                        use crate::models::const_val::[<ECHO_ $base_name:snake:upper>];
                        use crate::models::const_val::[<ECHO_ $base_name:snake:upper _EXPIRE>];
                        let cookie = self
                            .[<$base_name _jar>]()
                            .get([<ECHO_ $base_name:snake:upper>])
                            .ok_or(MissingCookieError::[<$base_name:camel>])?;
                        let raw = b64_general_engine::URL_SAFE
                            .decode(cookie.value())
                            .map_err(SessionError::Base64Decode)?;
                        let sess: [<$base_name:camel SessionData>] = rmp_serde::from_slice(&raw)?;
                        if sess.session_id != *self.session_id() {
                            return Err(SessionError::InvalidSessionId(sess.session_id.clone()));
                        }
                        if sess.create_at.add([<ECHO_ $base_name:snake:upper _EXPIRE>]) <= time::OffsetDateTime::now_utc() {
                            return Err(SessionError::SessionExpired);
                        }
                        Ok(sess)
                    }
                }
            }
        )*
    };
}

auth_session!(
    basic_auth => (data = BasicAuthData, same_site = SameSite::Lax, biz_code = 11100),
    csrf_auth => (data = CsrfAuthData, same_site = SameSite::Strict, biz_code = 11200),
    pre_mfa_auth => (data = PreMfaAuthData, same_site = SameSite::Lax, biz_code = 11300),
    mfa_auth => (data = MfaAuthData, same_site = SameSite::Lax, biz_code = 11400),
);

impl<S> FromRequestParts<S> for SessionHelper
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> ApiResult<Self> {
        parts
            .extensions
            .get::<SessionHelper>()
            .cloned()
            .ok_or(internal!(
                "Cannot extract session. Is `SessionLayer` enabled?"
            ))
    }
}

#[derive(Clone)]
pub struct SessionLayer {
    state: Arc<EchoState>,
}

impl SessionLayer {
    pub fn new(state: Arc<EchoState>) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for SessionLayer {
    type Service = CookieManager<SessionService<S>>;

    fn layer(&self, inner: S) -> Self::Service {
        let session = SessionService {
            inner,
            state: self.state.clone(),
        };
        CookieManagerLayer::new().layer(session)
    }
}

#[derive(Clone)]
pub struct SessionService<S> {
    inner: S,
    state: Arc<EchoState>,
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for SessionService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let cookies = req
            .extensions()
            .get::<Cookies>()
            .cloned()
            .expect("Cannot extract cookies. That's impossible!");
        req.extensions_mut()
            .insert(SessionHelper::new(self.state.clone(), cookies));
        self.inner.call(req)
    }
}
