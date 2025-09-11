pub mod auth;
pub mod cache;
pub mod config;
pub mod db;

use auth::AuthState;
use cache::CacheState;
use config::AppConfig;
use db::DataBaseState;
use std::sync::Arc;

pub struct EchoState {
    pub db: DataBaseState,
    pub cache: CacheState,
    pub auth: AuthState,
    pub config: Arc<AppConfig>,
}
