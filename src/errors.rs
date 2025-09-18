#[derive(Debug, thiserror::Error)]
pub enum EchoError {
    #[error("{source}")]
    EchoPM {
        #[from]
        source: crate::gladiator::GladiatorPipelineError,
        #[backtrace]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("{source}")]
    DataBase {
        #[from]
        source: crate::services::states::db::DataBaseError,
        #[backtrace]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("{source}")]
    IOError {
        #[from]
        source: std::io::Error,
        #[backtrace]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("{source}")]
    VarError {
        #[from]
        source: std::env::VarError,
        #[backtrace]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("{source}")]
    ConfigError {
        #[from]
        source: Box<figment::Error>,
        #[backtrace]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("Sqlx error: {0}")]
    SqlxError(#[from] sqlx::Error),
}

pub type EchoResult<T> = Result<T, EchoError>;

pub trait EchoBusinessErrCode {
    fn code(&self) -> Option<u32>;
}

impl<T: EchoBusinessErrCode + ?Sized> EchoBusinessErrCode for &T {
    #[inline]
    fn code(&self) -> Option<u32> {
        (**self).code()
    }
}

impl<T: EchoBusinessErrCode + ?Sized> EchoBusinessErrCode for &mut T {
    #[inline]
    fn code(&self) -> Option<u32> {
        (**self).code()
    }
}

impl EchoBusinessErrCode for core::convert::Infallible {
    #[inline]
    fn code(&self) -> Option<u32> {
        None
    }
}

impl EchoBusinessErrCode for std::time::SystemTimeError {
    #[inline]
    fn code(&self) -> Option<u32> {
        None
    }
}

impl EchoBusinessErrCode for time::error::ComponentRange {
    #[inline]
    fn code(&self) -> Option<u32> {
        None
    }
}
