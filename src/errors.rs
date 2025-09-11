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
