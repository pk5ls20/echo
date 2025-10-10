mod dyn_setting;
mod echo;
mod invite_code;
mod mfa;
mod permission;
mod resources;
mod token;
mod users;

use crate::services::states::db::dyn_setting::DynSettingsRepo;
use crate::services::states::db::echo::EchoRepo;
use crate::services::states::db::invite_code::InviteCodeRepo;
use crate::services::states::db::mfa::MfaRepo;
use crate::services::states::db::permission::PermissionRepo;
use crate::services::states::db::resources::ResourceRepo;
use crate::services::states::db::token::TokenRepo;
use crate::services::states::db::users::UsersRepo;
use crate::utils::smart_to_string::SmartStringError;
use echo_macros::EchoBusinessError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use sqlx::sqlite::SqliteQueryResult;
use sqlx::{Acquire, Executor, Pool, Sqlite, SqliteConnection, SqlitePool};
use std::backtrace::Backtrace;
use std::fmt::Debug;
use std::sync::Arc;

#[derive(Debug, thiserror::Error, EchoBusinessError)]
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
pub struct NeverRetPageQueryItem {
    idx: i64,
}

impl NeverRetPageQueryItem {
    pub fn from_range(start: i64, size: i64) -> Vec<Self> {
        (start..start + size)
            .map(|idx| NeverRetPageQueryItem { idx })
            .collect()
    }
}

impl PageQueryCursor for NeverRetPageQueryItem {
    fn cursor_field(&self) -> i64 {
        self.idx
    }
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

impl<T> PageQueryResult<T>
where
    T: Debug + Serialize + DeserializeOwned,
{
    pub fn swap_items<S>(self, items: Vec<S>) -> PageQueryResult<S>
    where
        S: Debug + Serialize + DeserializeOwned,
    {
        PageQueryResult {
            items,
            has_more: self.has_more,
            next_cursor: self.next_cursor,
        }
    }
}

impl PageQueryBinder {
    pub async fn query_page_ctx<T, F, Fut>(self, query_fn: F) -> DataBaseResult<PageQueryResult<T>>
    where
        T: PageQueryCursor,
        F: FnOnce(PageQueryInner) -> Fut,
        Fut: Future<Output = Result<Vec<T>, sqlx::Error>>,
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

pub struct DataBaseExecutor<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    inner: &'a mut E,
}

impl<'a, E> DataBaseExecutor<'a, E>
where
    for<'c> &'c mut E: Executor<'c, Database = Sqlite>,
{
    #[inline]
    pub fn dyn_settings(&mut self) -> DynSettingsRepo<'_, E> {
        DynSettingsRepo {
            inner: &mut *self.inner,
        }
    }

    #[inline]
    pub fn echo(&mut self) -> EchoRepo<'_, E> {
        EchoRepo {
            inner: &mut *self.inner,
        }
    }

    #[inline]
    pub fn invite_code(&mut self) -> InviteCodeRepo<'_, E> {
        InviteCodeRepo {
            inner: &mut *self.inner,
        }
    }

    #[inline]
    pub fn mfa(&mut self) -> MfaRepo<'_, E> {
        MfaRepo {
            inner: &mut *self.inner,
        }
    }

    #[inline]
    pub fn permission(&mut self) -> PermissionRepo<'_, E> {
        PermissionRepo {
            inner: &mut *self.inner,
        }
    }

    #[inline]
    pub fn resources(&mut self) -> ResourceRepo<'_, E> {
        ResourceRepo {
            inner: &mut *self.inner,
        }
    }

    #[inline]
    pub fn token(&mut self) -> TokenRepo<'_, E> {
        TokenRepo {
            inner: &mut *self.inner,
        }
    }

    #[inline]
    pub fn users(&mut self) -> UsersRepo<'_, E> {
        UsersRepo {
            inner: &mut *self.inner,
        }
    }
}

pub type EchoDatabaseExecutor<'a> = DataBaseExecutor<'a, SqliteConnection>;

#[derive(Clone)]
pub struct DataBaseState {
    pool: Arc<Pool<Sqlite>>,
}

// TODO: RustRover currently cannot infer the parameter types of asynchronous closure functions
// TODO: we must manually annotate them. So fxxk you, JetBrains!
// ref: https://youtrack.jetbrains.com/issue/RUST-18759
impl DataBaseState {
    pub async fn single<F, R, E>(&self, f: F) -> Result<R, E>
    where
        for<'q> F: AsyncFnOnce(EchoDatabaseExecutor<'q>) -> Result<R, E> + Send,
        R: Send,
        E: Send + From<DataBaseError>,
    {
        let mut conn = self.pool.acquire().await.resolve()?;
        let exec = DataBaseExecutor { inner: &mut *conn };
        f(exec).await
    }

    pub async fn transaction<F, R, E>(&self, f: F) -> Result<R, E>
    where
        for<'q> F: AsyncFnOnce(EchoDatabaseExecutor<'q>) -> Result<R, E> + Send,
        R: Send,
        E: Send + From<DataBaseError>,
    {
        let mut conn = self.pool.acquire().await.resolve()?;
        let mut tx = conn.begin().await.resolve()?;
        let exec = DataBaseExecutor { inner: &mut *tx };
        let out = f(exec).await;
        match out {
            Ok(val) => {
                tx.commit().await.resolve()?;
                Ok(val)
            }
            Err(err) => {
                tx.rollback().await.resolve()?;
                Err(err)
            }
        }
    }
}

impl DataBaseState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }

    pub async fn close_conn(&self) {
        self.pool.close().await;
    }
}
