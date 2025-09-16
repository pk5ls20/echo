use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct GeneralResponse<T> {
    pub msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> GeneralResponse<T>
where
    (StatusCode, Json<GeneralResponse<T>>): IntoResponse,
{
    pub fn new(msg: impl Into<String>, data: Option<T>) -> Self {
        Self {
            msg: msg.into(),
            data,
        }
    }

    pub fn into_response(self, status: StatusCode) -> Response {
        <(StatusCode, Json<Self>) as IntoResponse>::into_response((status, Json(self)))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    #[serde(skip)]
    pub status: StatusCode,
    pub message: String,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    fn api_error_inner<E, T>(
        status: StatusCode,
        err: Option<E>,
        msg: Option<T>,
        fallback_msg: &str,
    ) -> Self
    where
        E: std::error::Error,
        T: Into<String>,
    {
        let err_user_msg = msg.map(|m| m.into()).unwrap_or_else(|| fallback_msg.into());
        let mut err_emit_msg = format!("An api error occurred!\nmsg: {}", &err_user_msg);
        if let Some(err) = err {
            err_emit_msg.push_str(&format!("\nerror: {}", err));
            tracing::error!("{}\nStack: {:?}", &err_emit_msg, &err);
        }
        Self {
            status,
            message: err_user_msg,
        }
    }
}

impl From<DataBaseError> for ApiError {
    //noinspection ALL: so fxxk u rustrover!
    fn from(e: DataBaseError) -> Self {
        internal!(e, "Database error")
    }
}

macro_rules! define_api_error {
    ($fn_name:ident, $http_status:expr, $fallback_msg:expr) => {
        impl ApiError {
            #[inline]
            pub fn $fn_name<E, T>(err: Option<E>, msg: Option<T>) -> Self
            where
                E: ::std::error::Error,
                T: Into<String>,
            {
                Self::api_error_inner($http_status, err, msg, $fallback_msg)
            }
        }
        macro_rules! $fn_name {
            (err = $err: expr) => {
                $crate::models::api::ApiError::$fn_name(Some($err), None::<&str>)
            };
            (msg = $msg: expr) => {
                $crate::models::api::ApiError::$fn_name::<::std::convert::Infallible, _>(
                    None,
                    Some($msg),
                )
            };
            ($msg: literal) => {
                $crate::models::api::ApiError::$fn_name::<::std::convert::Infallible, _>(
                    None,
                    Some($msg),
                )
            };
            ($msg: expr) => {
                $crate::models::api::ApiError::$fn_name::<::std::convert::Infallible, _>(
                    None,
                    Some($msg),
                )
            };
            ($err: expr,$msg: expr) => {
                $crate::models::api::ApiError::$fn_name(Some($err), Some($msg))
            };
        }
        #[allow(unused_imports)]
        pub(crate) use $fn_name;
    };
}

define_api_error!(bad_request, StatusCode::BAD_REQUEST, "Bad Request");
define_api_error!(unauthorized, StatusCode::UNAUTHORIZED, "Unauthorized");
define_api_error!(conflict, StatusCode::CONFLICT, "Conflict");
define_api_error!(
    internal,
    StatusCode::INTERNAL_SERVER_ERROR,
    "Internal Server Error"
);

macro_rules! general_json_res {
    ($msg:literal) => {
        Json(GeneralResponse::new($msg, None))
    };
    ($msg:literal, $data:expr) => {
        Json(GeneralResponse::new($msg, Some($data)))
    };
}

use crate::services::states::db::DataBaseError;
pub(crate) use general_json_res;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        let body = Json(self);
        (status, body).into_response()
    }
}

pub mod prelude {
    pub use super::{ApiError, ApiResult, GeneralResponse};
    pub(crate) use crate::models::api::general_json_res;
    pub(crate) use crate::models::api::{bad_request, conflict, internal, unauthorized};
}
