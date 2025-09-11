#![allow(dead_code)]
#![feature(bool_to_result)]
#![feature(error_generic_member_access)]
#![feature(if_let_guard)]

mod errors;
mod gladiator;
mod layers;
mod models;
mod routers;
mod services;
mod utils;

use crate::errors::EchoError;
use crate::routers::router;
use clap::Parser;
use services::states::EchoState;
use services::states::auth::AuthState;
use services::states::cache::CacheState;
use services::states::config::AppConfig;
use services::states::db::DataBaseState;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

#[cfg(all(target_os = "windows", feature = "alternative-allocator"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(all(
    any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ),
    feature = "alternative-allocator"
))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn tracing_init(level: &str) {
    use std::io::stdout;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{EnvFilter, Layer};
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(stdout)
        .with_filter(EnvFilter::new(level));
    tracing_subscriber::registry().with(fmt_layer).init();
}

#[cfg_attr(test, ctor::ctor)]
fn init() {
    tracing_init("info,echo=debug");
}

pub mod shadow {
    use shadow_rs::shadow;
    shadow!(build_info);
}

#[derive(clap::Parser, Debug)]
#[clap(
    name = "echo",
    version = shadow::build_info::VERSION,
    long_version = shadow::build_info::CLAP_LONG_VERSION
)]
pub struct Cli {
    #[clap(short, long, help = "Path to config file", default_value = "echo.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Arc::new(AppConfig::load(&cli.config)?);
    tracing_init(&config.common.log_level);
    #[cfg_attr(not(feature = "sqlcipher"), allow(unused_mut))]
    let mut sqlx_opt = SqliteConnectOptions::from_str(&config.db.db_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);
    #[cfg(feature = "sqlcipher")]
    {
        use anyhow::{Context, ensure};
        use rpassword::prompt_password;
        use std::io::{IsTerminal, stdout};
        let cipher_cfg = &config.db.cipher;
        if cipher_cfg.encrypt {
            tracing::info!("SQLCipher feature & encrypt enabled, applying pragmas...");
            let key = config
                .db
                .cipher
                .password
                .as_deref()
                .map(str::to_owned)
                .map(Ok)
                .unwrap_or_else(|| {
                    tracing::warn!("No password provided from config, prompting...");
                    ensure!(
                        stdout().is_terminal(),
                        "No password provided from config and not running in a terminal!"
                    );
                    prompt_password("Enter the database password:")
                        .context("failed to read password")
                })?;
            sqlx_opt = sqlx_opt
                .pragma("key", format!("'{key}'"))
                .pragma("cipher_page_size", cipher_cfg.cipher_page_size.to_string())
                .pragma("kdf_iter", cipher_cfg.kdf_iter.to_string())
                .pragma(
                    "cipher_hmac_algorithm",
                    cipher_cfg.cipher_hmac_algorithm.as_ref().to_owned(),
                )
                .pragma(
                    "cipher_default_kdf_algorithm",
                    cipher_cfg.cipher_default_kdf_algorithm.as_ref().to_owned(),
                )
                .pragma("cipher", cipher_cfg.cipher.as_ref().to_owned());
        }
    }
    let sqlx_pool = SqlitePoolOptions::new()
        .max_connections(config.db.sqlite_connection_nums)
        .connect_with(sqlx_opt)
        .await
        .map_err(EchoError::SqlxError)?;
    #[cfg(feature = "migrate")]
    {
        tracing::info!("Preparing to run embed migrations...");
        sqlx::migrate!("./migrations")
            .run(&sqlx_pool)
            .await
            .map_err(|e| {
                tracing::error!("Failed to run migrations: {}", e);
                EchoError::SqlxError(e.into())
            })?;
        tracing::info!("Migrations completed successfully.");
    }
    let db = DataBaseState::new(sqlx_pool);
    tracing::info!("Initializing db dyn settings...");
    db.dyn_settings().initialise().await?;
    let cache = CacheState::new();
    let auth = AuthState::new();
    let addr = format!("{}:{}", config.common.host, config.common.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(
        "Starting server at {}:{}",
        config.common.host,
        config.common.port
    );
    let echo_state = Arc::new(EchoState {
        db,
        cache,
        auth,
        config,
    });
    axum::serve(listener, router(echo_state.clone()).await)
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut sigint =
                    signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
                let mut sigterm =
                    signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
                tokio::select! {
                    _ = sigint.recv() => {},
                    _ = sigterm.recv() => {},
                }
            }
            #[cfg(windows)]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            #[cfg(not(any(unix, windows)))]
            {
                tracing::warn!("Graceful shutdown is not supported on this platform.");
                futures::future::pending::<()>().await;
            }
            tracing::warn!("Received shutdown signal, shutting down gracefully...");
        })
        .await?;
    tracing::info!("Trying to close database connections...");
    match tokio::time::timeout(Duration::from_secs(15), echo_state.db.close_conn()).await {
        Ok(_) => tracing::info!("Database connections closed."),
        Err(_) => tracing::error!("Timed out while closing database connections."),
    }
    Ok(())
}
