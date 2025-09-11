use crate::shadow::build_info;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct BuildInfo {
    pub branch: &'static str,
    pub build_os: &'static str,
    pub build_time: &'static str,
    pub build_target: &'static str,
    pub build_rust_channel: &'static str,
    pub rust_version: &'static str,
    pub commit_hash: &'static str,
    pub commit_date: &'static str,
    pub commit_author: &'static str,
    pub commit_email: &'static str,
    pub pkg_version: &'static str,
}

pub const BUILD_INFO: BuildInfo = BuildInfo {
    branch: build_info::BRANCH,
    build_os: build_info::BUILD_OS,
    build_time: build_info::BUILD_TIME,
    build_target: build_info::BUILD_TARGET,
    build_rust_channel: build_info::BUILD_RUST_CHANNEL,
    rust_version: build_info::RUST_VERSION,
    commit_hash: build_info::COMMIT_HASH,
    commit_date: build_info::COMMIT_DATE,
    commit_author: build_info::COMMIT_AUTHOR,
    commit_email: build_info::COMMIT_EMAIL,
    pkg_version: build_info::PKG_VERSION,
};
