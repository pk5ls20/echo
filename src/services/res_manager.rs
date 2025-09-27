use crate::services::states::EchoState;
use echo_macros::EchoBusinessError;
use hmac::digest::core_api::CoreWrapper;
use hmac::{Hmac, HmacCore, Mac};
use serde::{Deserialize, Serialize};
use serde_with::{
    base64::{Base64, UrlSafe},
    serde_as,
};
use sha2::Sha256;
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use url::{Url, form_urlencoded};

#[derive(Debug, thiserror::Error, EchoBusinessError)]
pub enum ResManagerServiceError {
    #[error(transparent)]
    UrlSerde(#[from] serde_urlencoded::ser::Error),
    #[error(transparent)]
    UrlParse(#[from] url::ParseError),
    #[error(transparent)]
    RmpEncode(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    RmpDecode(#[from] rmp_serde::decode::Error),
    #[error(
        "Invalid HMAC key length! This is quite absurd, as the construction process of the \
    key is determined by code behaviour that has already been completed!"
    )]
    InvalidLength(#[from] hmac::digest::InvalidLength),
    #[error(transparent)]
    MacVerify(#[from] hmac::digest::MacError),
    #[error("Your sign has expired!")]
    SignExpired,
    #[error("Overflow occurred when calculating sign expiration time!")]
    SignExpOverflow,
    #[error(
        "The resource ID in the sign does not match the expected one! Expected {expected}, got {got}"
    )]
    ResIdNotMatch { expected: i64, got: i64 },
}

pub type ResManagerServiceResult<T> = Result<T, ResManagerServiceError>;

#[derive(Debug, Serialize, Deserialize)]
pub struct ExchangedResourceTag {
    pub sign_user_id: i64,
    pub sign_time: OffsetDateTime,
    pub exp_time: Duration,
    pub res_id: i64,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
pub struct ExchangedResourceItem {
    #[serde_as(as = "Base64<UrlSafe>")]
    #[serde(rename = "aira")]
    pub tag: Vec<u8>,
    #[serde_as(as = "Base64<UrlSafe>")]
    #[serde(rename = "sora")]
    pub sig: Vec<u8>,
}

impl ExchangedResourceItem {
    pub fn to_url(&self, base_url: Option<&str>) -> ResManagerServiceResult<String> {
        serde_urlencoded::to_string(self)
            .map_err(Into::into)
            .and_then(|qs| {
                base_url.map_or_else(
                    || Ok(format!("/?{qs}")),
                    |base| {
                        Url::parse(base).map_err(Into::into).map(|mut u| {
                            u.query_pairs_mut()
                                .extend_pairs(form_urlencoded::parse(qs.as_bytes()));
                            u.into()
                        })
                    },
                )
            })
    }
}

pub struct ResManagerService {
    state: Arc<EchoState>,
}

impl ResManagerService {
    pub fn new(state: Arc<EchoState>) -> Self {
        ResManagerService { state }
    }

    #[inline]
    fn get_mac(&self) -> ResManagerServiceResult<CoreWrapper<HmacCore<Sha256>>> {
        Ok(Hmac::<Sha256>::new_from_slice(
            self.state.auth.get_local_res_key(),
        )?)
    }

    pub fn sign(
        &self,
        user_id: i64,
        exp_time: Duration,
        res_id: i64,
    ) -> ResManagerServiceResult<ExchangedResourceItem> {
        let exchange_res = ExchangedResourceTag {
            sign_user_id: user_id,
            sign_time: OffsetDateTime::now_utc(),
            exp_time,
            res_id,
        };
        let exchange_res = rmp_serde::encode::to_vec(&exchange_res)?;
        let mut mac = self.get_mac()?;
        mac.update(&exchange_res);
        let sig = mac.finalize().into_bytes().to_vec();
        Ok(ExchangedResourceItem {
            tag: exchange_res,
            sig,
        })
    }

    pub fn verify(
        &self,
        res_id: i64,
        item: &ExchangedResourceItem,
    ) -> ResManagerServiceResult<ExchangedResourceTag> {
        let mut mac = self.get_mac()?;
        mac.update(&item.tag);
        mac.verify_slice(&item.sig)
            .map_err(ResManagerServiceError::MacVerify)?;
        let tag = rmp_serde::decode::from_slice::<ExchangedResourceTag>(&item.tag)?;
        if tag.res_id != res_id {
            return Err(ResManagerServiceError::ResIdNotMatch {
                expected: tag.res_id,
                got: res_id,
            });
        }
        let sign_time = tag
            .sign_time
            .checked_add(tag.exp_time)
            .ok_or(ResManagerServiceError::SignExpOverflow)?;
        if sign_time < OffsetDateTime::now_utc() {
            return Err(ResManagerServiceError::SignExpired);
        }
        Ok(tag)
    }
}
