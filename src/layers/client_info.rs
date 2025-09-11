use crate::models::api::prelude::*;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{Request, Response, header};
use std::task::{Context, Poll};
use tower::{Layer, Service};

#[derive(Clone, Debug)]
pub struct ClientInfo {
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}

impl ClientInfo {
    fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        let user_agent = headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let ip_address = headers
            .get("x-real-ip")
            .or_else(|| headers.get("x-forwarded-for"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
        Self {
            user_agent,
            ip_address,
        }
    }
}

impl<S> FromRequestParts<S> for ClientInfo
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> ApiResult<Self> {
        parts
            .extensions
            .get::<ClientInfo>()
            .cloned()
            .ok_or(internal!(
                "Cannot extract client info. Is `ClientInfoLayer` enabled?"
            ))
    }
}

#[derive(Clone, Default)]
pub struct ClientInfoLayer;

impl ClientInfoLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for ClientInfoLayer {
    type Service = ClientInfoService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ClientInfoService { inner }
    }
}

#[derive(Clone)]
pub struct ClientInfoService<S> {
    inner: S,
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for ClientInfoService<S>
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
        let info = ClientInfo::from_headers(req.headers());
        req.extensions_mut().insert(info);
        self.inner.call(req)
    }
}
