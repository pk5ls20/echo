use crate::get_batch_tuple_pure;
use crate::models::dyn_setting::{RpId, RpName, RpOrigin};
use crate::models::mfa::{
    NewTotpCredential, NewWebauthnCredential, WebAuthnKV, WebauthnCredential, WebauthnState,
};
use crate::services::states::cache::MokaExpiration;
use crate::services::states::db::DataBaseError;
use crate::services::states::EchoState;
use bytes::Bytes;
use echo_macros::EchoBusinessError;
use ph::fmph;
use rand::{rng, Rng};
use std::sync::Arc;
use time::Duration;
use totp_rs::{TotpUrlError, TOTP};
use url::Url;
use webauthn_rs::prelude::*;

#[derive(Debug, thiserror::Error, EchoBusinessError)]
pub enum TOTPError {
    #[error(transparent)]
    TotpUrl(#[from] TotpUrlError),
}

#[derive(Debug, thiserror::Error, EchoBusinessError)]
pub enum AuthnError {
    #[error("Failed to init WebAuthn service: {0}")]
    WebAuthnInit(String),
    #[error("Invalid Webauthn RP origin")]
    InvalidWebauthnRpOrigin,
    #[error("Invalid Webauthn state")]
    InvalidWebauthnState,
    #[error("Mismatch WebAuthnKV in same state!")]
    MismatchWebAuthnKV,
    #[error("WebAuthn error: {0}")]
    WebauthnOther(#[from] WebauthnError),
}

#[derive(Debug, thiserror::Error, EchoBusinessError)]
pub enum MFAServiceError {
    #[error("RMP decode error: {0}")]
    RmpDecode(#[from] rmp_serde::decode::Error),
    #[error("RMP encode error: {0}")]
    RmpEncode(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    Database(#[from] DataBaseError),
    #[error(transparent)]
    Totp(#[from] TOTPError),
    #[error(transparent)]
    Authn(#[from] AuthnError),
}

pub type MFAServiceResult<T> = Result<T, MFAServiceError>;

pub struct MFAService {
    state: Arc<EchoState>,
    webauthn: Webauthn,
}

impl MFAService {
    pub async fn new(state: Arc<EchoState>) -> MFAServiceResult<Self> {
        let dyn_setting_op = &state.db;
        let (rp_id, rp_origin, rp_name): (String, String, String) =
            get_batch_tuple_pure!(&dyn_setting_op, RpId, RpOrigin, RpName)
                .map_err(|e| AuthnError::WebAuthnInit(e.to_string()))?;
        let rp_origin = Url::parse(&rp_origin).map_err(|_| AuthnError::InvalidWebauthnRpOrigin)?;
        let webauthn = WebauthnBuilder::new(&rp_id, &rp_origin)
            .map_err(|e| MFAServiceError::Authn(AuthnError::WebauthnOther(e)))?
            .rp_name(&rp_name)
            .build()
            .map_err(|e| MFAServiceError::Authn(AuthnError::WebauthnOther(e)))?;
        Ok(Self { state, webauthn })
    }

    pub fn generate_totp(&self, account_name: impl Into<String>) -> MFAServiceResult<TOTP> {
        let secret_bytes = (0..30).map(|_| rng().random()).collect::<Vec<u8>>();
        TOTP::new(
            totp_rs::Algorithm::SHA512,
            6,
            1,
            30,
            secret_bytes,
            Some("Echo".to_string()),
            account_name.into(),
        )
        .map_err(|e| MFAServiceError::Totp(TOTPError::TotpUrl(e)))
    }

    pub fn load_totp(&self, totp_data: &[u8]) -> MFAServiceResult<TOTP> {
        rmp_serde::from_slice(totp_data).map_err(MFAServiceError::from)
    }

    pub fn save_totp(&self, user_id: i64, totp: &TOTP) -> MFAServiceResult<NewTotpCredential> {
        let data = rmp_serde::to_vec(totp).map_err(MFAServiceError::from)?;
        Ok(NewTotpCredential {
            user_id,
            totp_credential_data: data,
        })
    }

    pub async fn start_passkey_registration(
        &self,
        user_id: i64,
        user_name: &str,
        already_owned_passkey: Option<Vec<WebauthnCredential>>,
    ) -> MFAServiceResult<CreationChallengeResponse> {
        let exclude_credentials = already_owned_passkey
            .map(|creds| {
                creds
                    .into_iter()
                    .map(|c| {
                        rmp_serde::from_slice::<Passkey>(&c.credential_data)
                            .map(|data| data.cred_id().to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let user_unique_uuid = Uuid::new_v5(&Uuid::NAMESPACE_DNS, &user_id.to_be_bytes());
        let (ccr, reg_state) = self
            .webauthn
            .start_passkey_registration(user_unique_uuid, user_name, user_name, exclude_credentials)
            .map_err(AuthnError::from)?;
        let state = WebauthnState {
            user_name: user_name.to_string(),
            state: (user_unique_uuid, reg_state),
        };
        self.state
            .cache
            .set_passkey_reg_session(
                user_name.to_string(),
                (
                    MokaExpiration::new(Duration::minutes(5)),
                    Bytes::from(rmp_serde::to_vec(&state)?),
                ),
            )
            .await;
        Ok(ccr)
    }

    pub async fn finish_passkey_registration(
        &self,
        user_id: i64,
        user_name: impl Into<String>,
        reg: RegisterPublicKeyCredential,
    ) -> MFAServiceResult<NewWebauthnCredential> {
        let (_, info) = self
            .state
            .cache
            .get_passkey_reg_session(user_name.into())
            .await
            .ok_or(MFAServiceError::Authn(AuthnError::InvalidWebauthnState))?;
        let info = rmp_serde::from_slice::<WebauthnState<(Uuid, PasskeyRegistration)>>(&info)
            .map_err(MFAServiceError::from)?;
        let res = self
            .webauthn
            .finish_passkey_registration(&reg, &info.state.1)
            .map_err(AuthnError::from)?;
        Ok(NewWebauthnCredential {
            user_id,
            user_unique_uuid: info.state.0,
            user_name: info.user_name,
            user_display_name: None,
            credential_data: rmp_serde::to_vec(&res).map_err(MFAServiceError::from)?,
        })
    }

    pub async fn start_passkey_authentication(
        &self,
        user_id: i64,
        user_name: impl Into<String>,
        already_owned_passkey: Vec<WebauthnCredential>,
    ) -> MFAServiceResult<RequestChallengeResponse> {
        let user_name = user_name.into();
        let idx2id = already_owned_passkey
            .iter()
            .map(|it| it.id)
            .collect::<Vec<_>>();
        let already_owned_passkey = already_owned_passkey
            .iter()
            .map(|creds| rmp_serde::from_slice::<Passkey>(&creds.credential_data))
            .collect::<Result<Vec<_>, _>>()?;
        let (rcr, auth_state) = self
            .webauthn
            .start_passkey_authentication(&already_owned_passkey)
            .map_err(AuthnError::from)?;
        let fmph_f = fmph::GOFunction::from(
            already_owned_passkey
                .iter()
                .map(|c| c.cred_id().as_ref())
                .collect::<Vec<&[u8]>>(),
        );
        let state = WebauthnState {
            user_name: user_name.clone(),
            state: auth_state,
        };
        let kv = WebAuthnKV { f: fmph_f, idx2id };
        let exp = Duration::minutes(5);
        self.state
            .cache
            .set_passkey_auth_session(
                user_name.clone(),
                (
                    MokaExpiration::new(exp),
                    Bytes::from(rmp_serde::to_vec(&state)?),
                ),
            )
            .await;
        self.state
            .cache
            .set_passkey_ph_session(user_id, (MokaExpiration::new(exp), Arc::new(kv)))
            .await;
        Ok(rcr)
    }

    pub async fn finish_passkey_authentication(
        &self,
        user_id: i64,
        user_name: impl Into<String>,
        auth: PublicKeyCredential,
    ) -> MFAServiceResult<(AuthenticationResult, i64)> {
        let (_, info) = self
            .state
            .cache
            .get_passkey_auth_session(user_name.into())
            .await
            .ok_or(MFAServiceError::Authn(AuthnError::InvalidWebauthnState))?;
        let (_, kv) = self
            .state
            .cache
            .get_passkey_ph_session(user_id)
            .await
            .ok_or(MFAServiceError::Authn(AuthnError::InvalidWebauthnState))?;
        let info = rmp_serde::from_slice::<WebauthnState<PasskeyAuthentication>>(&info)
            .map_err(MFAServiceError::from)?;
        let res = self
            .webauthn
            .finish_passkey_authentication(&auth, &info.state)
            .map_err(AuthnError::from)?;
        let idx =
            kv.f.get(auth.raw_id.as_ref())
                .ok_or(MFAServiceError::Authn(AuthnError::MismatchWebAuthnKV))?;
        let id = kv
            .idx2id
            .get(idx as usize)
            .ok_or(MFAServiceError::Authn(AuthnError::MismatchWebAuthnKV))?;
        Ok((res, *id))
    }
}
