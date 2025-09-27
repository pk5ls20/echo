use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct CommonConfig {
    pub host: Cow<'static, str>,
    pub port: usize,
    pub log_level: Cow<'static, str>,
    pub concurrency_limit: usize,
}

impl Default for CommonConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 18200,
            log_level: "info,echo=debug".into(),
            concurrency_limit: 128,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataBaseConfig {
    pub db_url: Cow<'static, str>,
    pub sqlite_connection_nums: u32,
    #[cfg(feature = "sqlcipher")]
    #[serde(flatten)]
    pub cipher: DataBaseEncryptConfig,
}

impl Default for DataBaseConfig {
    fn default() -> Self {
        Self {
            db_url: "sqlite://data/echo.db".into(),
            sqlite_connection_nums: 10,
            #[cfg(feature = "sqlcipher")]
            cipher: DataBaseEncryptConfig::default(),
        }
    }
}

#[cfg(feature = "sqlcipher")]
#[derive(Debug, Serialize, Deserialize)]
pub struct DataBaseEncryptConfig {
    pub encrypt: bool,
    pub password: Option<Cow<'static, str>>,
    pub cipher_page_size: u32,
    pub kdf_iter: u32,
    pub cipher_hmac_algorithm: Cow<'static, str>,
    pub cipher_default_kdf_algorithm: Cow<'static, str>,
    pub cipher: Cow<'static, str>,
}

#[cfg(feature = "sqlcipher")]
impl Default for DataBaseEncryptConfig {
    fn default() -> Self {
        Self {
            encrypt: false,
            password: None,
            cipher_page_size: 4096,
            kdf_iter: 256000,
            cipher_hmac_algorithm: "HMAC_SHA512".into(),
            cipher_default_kdf_algorithm: "PBKDF2_HMAC_SHA512".into(),
            cipher: "'aes-256-cbc'".into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResourceConfig {
    pub flush_stream_size: NonZeroU32,
    pub local_storage_path: PathBuf,
    pub tmp_file_path: Option<PathBuf>,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            flush_stream_size: NonZeroU32::new(256 * 1024).unwrap(), // 256 kb
            local_storage_path: "data/uploads".into(),
            tmp_file_path: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PerfConfig {
    pub user_cache_capacity: usize,
    pub res_cache_capacity: usize,
    pub dyn_setting_cache_capacity: usize,
}

impl Default for PerfConfig {
    fn default() -> Self {
        Self {
            user_cache_capacity: 5,
            res_cache_capacity: 50,
            dyn_setting_cache_capacity: 50,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub common: CommonConfig,
    pub db: DataBaseConfig,
    pub resource: ResourceConfig,
    pub perf: PerfConfig,
}

impl AppConfig {
    pub fn load(cfg_path: &str) -> Result<Self, Box<figment::Error>> {
        let mut figment = Figment::from(Serialized::defaults(AppConfig::default()))
            .merge(Env::prefixed("ECHO_").split("__").global());
        if Path::new(cfg_path).exists() {
            figment = figment.merge(Toml::file(cfg_path));
        }
        figment.extract().map_err(Into::into)
    }
}
