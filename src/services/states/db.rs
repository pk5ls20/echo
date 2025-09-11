mod dyn_setting;
mod echo;
mod invite_code;
mod mfa;
mod permission;
mod resources;
mod token;
mod users;

use crate::services::states::db::dyn_setting::DynSettingRepo;
use crate::services::states::db::echo::EchoRepo;
use crate::services::states::db::invite_code::InviteCodeRepo;
use crate::services::states::db::mfa::MfaRepo;
use crate::services::states::db::permission::PermissionRepo;
use crate::services::states::db::resources::ResourceRepo;
use crate::services::states::db::token::TokenRepo;
use crate::services::states::db::users::UserRepo;
use crate::utils::smart_to_string::SmartStringError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteQueryResult;
use std::backtrace::Backtrace;
use std::fmt::Debug;

#[derive(Debug, thiserror::Error)]
pub enum DataBaseError {
    #[error("{source}")]
    SerdeJson {
        #[from]
        source: serde_json::Error,
        #[backtrace]
        backtrace: Backtrace,
    },
    #[error("Row not found!")]
    RowNotFound(#[backtrace] Backtrace),
    #[error("No affected rows!")]
    NoAffectedRows(#[backtrace] Backtrace),
    #[error("Unique violation error! code: {code:?}, msg: {msg}")]
    UniqueViolation {
        code: Option<String>,
        msg: String,
        #[backtrace]
        backtrace: Backtrace,
    },
    #[error("Foreign key violation error! code: {code:?}, msg: {msg}")]
    ForeignKeyViolation {
        code: Option<String>,
        msg: String,
        #[backtrace]
        backtrace: Backtrace,
    },
    #[error("Internal error: {msg}")]
    Internal {
        msg: &'static str,
        #[backtrace]
        backtrace: Backtrace,
    },
    #[error("sqlx error: {0}")]
    SqlxOther(#[from] sqlx::Error),
    #[error("DynSetting parse error: {err}")]
    DynSettingParse {
        #[from]
        err: SmartStringError,
        #[backtrace]
        backtrace: Backtrace,
    },
}

pub trait PageQueryCursor: Debug + Serialize + DeserializeOwned {
    fn cursor_field(&self) -> i64;
}

#[serde_inline_default]
#[derive(Debug, Serialize, Deserialize)]
pub struct PageQueryBinder {
    pub start_after: i64,
    #[serde_inline_default(20)]
    pub page_size: u32,
}

pub struct PageQueryInner {
    pub start_after: i64,
    pub limit: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: DeserializeOwned"))]
pub struct PageQueryResult<T>
where
    T: Debug + Serialize + DeserializeOwned,
{
    pub items: Vec<T>,
    pub has_more: bool,
    pub next_cursor: Option<i64>,
}

impl PageQueryBinder {
    pub async fn query_page_ctx<T, F, Fut>(self, query_fn: F) -> DataBaseResult<PageQueryResult<T>>
    where
        T: PageQueryCursor,
        F: FnOnce(PageQueryInner) -> Fut,
        Fut: Future<Output = Result<Vec<T>, sqlx::Error>> + Send,
    {
        let limit = self.page_size + 1;
        let inner = PageQueryInner {
            start_after: self.start_after,
            limit,
        };
        let mut res = query_fn(inner).await.resolve()?;
        let res = match res.len() {
            n if n > 1 && n == (self.page_size + 1) as usize => {
                res.truncate(self.page_size as usize);
                // SAFETY: n if n > 1 && n == (self.page_size + 1) as usize
                let next_cursor = Some(res.last().unwrap().cursor_field());
                PageQueryResult {
                    items: res,
                    has_more: true,
                    next_cursor,
                }
            }
            _ => PageQueryResult {
                items: res,
                has_more: false,
                next_cursor: None,
            },
        };
        Ok(res)
    }
}

pub trait SqliteBaseResultExt<T> {
    fn resolve(self) -> DataBaseResult<T>;
}

impl<T> SqliteBaseResultExt<T> for Result<T, sqlx::Error> {
    fn resolve(self) -> DataBaseResult<T> {
        match self {
            Ok(result) => Ok(result),
            Err(sqlx::Error::RowNotFound) => Err(DataBaseError::RowNotFound(Backtrace::capture())),
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                Err(DataBaseError::UniqueViolation {
                    code: e.code().map(|c| c.to_string()),
                    msg: e.message().to_string(),
                    backtrace: Backtrace::capture(),
                })
            }
            Err(sqlx::Error::Database(e)) if e.is_foreign_key_violation() => {
                Err(DataBaseError::ForeignKeyViolation {
                    code: e.code().map(|c| c.to_string()),
                    msg: e.message().to_string(),
                    backtrace: Backtrace::capture(),
                })
            }
            Err(e) => Err(DataBaseError::SqlxOther(e)),
        }
    }
}

pub trait SqliteQueryResultExt {
    fn resolve_affected(self) -> DataBaseResult<SqliteQueryResult>;
}

impl SqliteQueryResultExt for Result<SqliteQueryResult, sqlx::Error> {
    fn resolve_affected(self) -> DataBaseResult<SqliteQueryResult> {
        match self {
            Ok(res) if res.rows_affected() == 0 => {
                Err(DataBaseError::NoAffectedRows(Backtrace::capture()))
            }
            other => other.resolve(),
        }
    }
}

pub type DataBaseResult<T> = Result<T, DataBaseError>;

pub struct DataBaseState {
    pool: SqlitePool,
}

impl DataBaseState {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn users(&self) -> UserRepo<'_> {
        UserRepo::new(&self.pool)
    }

    pub fn echos(&self) -> EchoRepo<'_> {
        EchoRepo::new(&self.pool)
    }

    pub fn resources(&self) -> ResourceRepo<'_> {
        ResourceRepo::new(&self.pool)
    }

    pub fn permissions(&self) -> PermissionRepo<'_> {
        PermissionRepo::new(&self.pool)
    }

    pub fn tokens(&self) -> TokenRepo<'_> {
        TokenRepo::new(&self.pool)
    }

    pub fn invite_code(&self) -> InviteCodeRepo<'_> {
        InviteCodeRepo::new(&self.pool)
    }

    pub fn dyn_settings(&self) -> DynSettingRepo<'_> {
        DynSettingRepo::new(&self.pool)
    }

    pub fn mfa(&self) -> MfaRepo<'_> {
        MfaRepo::new(&self.pool)
    }

    pub async fn close_conn(&self) {
        self.pool.close().await;
    }
}
