use crate::services::states::EchoState;
use crate::services::states::cache::MokaExpiration;
use abv::bv2av;
use echo_macros::{EchoBusinessError, EchoExt};
use leptos::prelude::*;
use markup5ever::Attribute;
use std::cell::Ref;
use std::sync::Weak as WeakArc;
use time::Duration;
use uuid::Uuid;

// TODO: zero-copy error key display
#[derive(Debug, Eq, PartialEq, thiserror::Error, EchoBusinessError)]
pub enum EchoExtError {
    #[error("Failed to upgrade weak arc")]
    ArcUpgradeError,
    #[error("Failed to convert extension id to usize")]
    ExtIdTransUsizeError,
    #[error("Fragment dom missing child")]
    FragDomMissingChild,
    #[error("Unknown extension id: {0}")]
    UnknownExtId(u32),
    #[error("Meta key not exist: {0}")]
    MetaKeyNotExist(String),
    #[error("Evaluate key exist: {0}")]
    EvaluateKeyExist(String),
    #[error("Custom validation error! key: {0}, err: {1}")]
    CustomValidationError(String, &'static str),
}

pub type EchoExtResult<T> = Result<T, EchoExtError>;

pub(super) trait EchoExtMeta {
    const ID: u32;
    const FUZZ_H: u32 = 200;
    const FUZZ_W: u32 = 300;
    const META_KEY: Option<phf::Set<&'static str>>;
    const EVALUATE_KEY: Option<phf::Set<&'static str>>;
}

pub(super) trait EchoExtHandler<'a>: EchoExtMeta {
    fn get_from_attr(
        attr: &'a Ref<'a, Vec<Attribute>>,
        key: &'a str,
        prefix: &'a str,
    ) -> EchoExtResult<&'a str> {
        attr.iter()
            .rev()
            .find_map(|a| {
                let name = a.name.local.as_ref();
                name.strip_prefix(prefix)
                    .filter(|rest| *rest == key)
                    .map(|_| a.value.as_ref())
            })
            .ok_or(EchoExtError::MetaKeyNotExist(key.to_string()))
    }

    fn get_meta_from_attr(
        attr: &'a Ref<'a, Vec<Attribute>>,
        key: &'a str,
    ) -> EchoExtResult<&'a str> {
        Self::get_from_attr(attr, key, "echo-ext-meta-")
    }

    fn validate_attr(attr: &'a Ref<'a, Vec<Attribute>>) -> EchoExtResult<()> {
        const PREFIX: &str = "echo-ext-meta-";
        let names_stripped = || {
            attr.iter()
                .filter_map(|a| a.name.local.as_ref().strip_prefix(PREFIX))
        };
        let stripped = names_stripped().collect::<Vec<_>>();
        if let Some(meta) = Self::META_KEY
            && let Some(&missing) = meta.iter().find(|&&need| !stripped.contains(&need))
        {
            return Err(EchoExtError::MetaKeyNotExist(missing.to_string()));
        }
        if let Some(eval) = Self::EVALUATE_KEY
            && let Some(&hit) = stripped.iter().find(|&&rest| eval.contains(rest))
        {
            return Err(EchoExtError::EvaluateKeyExist(hit.to_string()));
        }
        Self::custom_validate_attr(attr)?;
        Ok(())
    }

    fn custom_validate_attr(_: &'a Ref<'a, Vec<Attribute>>) -> EchoExtResult<()> {
        Ok(())
    }

    fn extract(ctx: WeakArc<EchoState>, attr: &'a Ref<'a, Vec<Attribute>>) -> EchoExtResult<Self>
    where
        Self: Sized;
}

pub(super) trait EchoExtRender<'a>: EchoExtHandler<'a> {
    fn render(&self) -> impl IntoView;
}

#[derive(Debug, EchoExt)]
#[echo_ext(id = 1)]
pub(super) struct EchoResourceExt<'a> {
    res_id: &'a str,
    #[eval]
    res_sign: Uuid,
}

impl<'a> EchoExtHandler<'a> for EchoResourceExt<'a> {
    fn extract(ctx: WeakArc<EchoState>, attr: &'a Ref<'a, Vec<Attribute>>) -> EchoExtResult<Self> {
        let res_id = Self::get_meta_from_attr(attr, "res-id")?;
        let res_sign = Uuid::new_v4();
        // This is counterintuitive (and likely performs poorly with poor semantics).
        // I think it's time we switch to a synchronous cache to avoid this.
        // FIXME: Replace the asynchronous cache with a synchronous version.
        tokio::spawn({
            let res_id = res_id.to_string();
            let ctx = ctx.upgrade().ok_or(EchoExtError::ArcUpgradeError)?;
            async move {
                ctx.cache
                    .set_res_sign(
                        res_id,
                        (MokaExpiration::new(Duration::minutes(5)), res_sign),
                    )
                    .await;
            }
        });
        Ok(Self { res_id, res_sign })
    }
}

impl<'a> EchoExtRender<'a> for EchoResourceExt<'a> {
    fn render(&self) -> impl IntoView {
        let src = format!("/api/v1/resource?id={}&qwq={}", self.res_id, self.res_sign);
        view! { <img src=src /> }
    }
}

#[derive(Debug, EchoExt)]
#[echo_ext(id = 2)]
pub(super) struct BiliVideoExt<'a> {
    /// Original av/bv id
    vid: &'a str,
    autoplay: bool,
    simple: bool,
    #[eval]
    av_id: u64,
    // TODO: video page (P1, P2, ...)
    // TODO: In practice, adding `video_page` itself is straightforward, but implementing it as
    // TODO: `Option<video_page>` currently lacks detailed specifications in the current standard.
}

pub(super) enum BiliVid<'a> {
    AV(u64),
    BV(&'a str),
}

impl<'a> BiliVideoExt<'a> {
    fn av_or_bv(s: &'a str) -> Option<BiliVid<'a>> {
        s.get(..2)
            .and_then(|p| match p.to_ascii_lowercase().as_str() {
                "av" => s[2..].parse::<u64>().ok().map(BiliVid::AV),
                "bv" => Some(BiliVid::BV(s)),
                _ => None,
            })
    }
}

impl<'a> EchoExtHandler<'a> for BiliVideoExt<'a> {
    fn custom_validate_attr(attr: &'a Ref<'a, Vec<Attribute>>) -> EchoExtResult<()> {
        let vid = Self::get_meta_from_attr(attr, "vid")?;
        if Self::av_or_bv(vid).is_none() {
            return Err(EchoExtError::CustomValidationError(
                vid.to_string(),
                "not a valid av/bv id",
            ));
        }
        Ok(())
    }

    fn extract(_: WeakArc<EchoState>, attr: &'a Ref<'a, Vec<Attribute>>) -> EchoExtResult<Self> {
        let vid = Self::get_meta_from_attr(attr, "vid")?;
        let vid_enum = Self::av_or_bv(vid).ok_or(EchoExtError::CustomValidationError(
            vid.to_string(),
            "not a valid av/bv id",
        ))?;
        let av_id = match vid_enum {
            BiliVid::AV(id) => id,
            BiliVid::BV(bv) => bv2av(bv).map_err(|_| {
                EchoExtError::CustomValidationError(bv.to_string(), "failed to convert bv to av")
            })?,
        };
        let autoplay = Self::get_meta_from_attr(attr, "autoplay")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let simple = Self::get_meta_from_attr(attr, "simple")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Ok(Self {
            vid,
            autoplay,
            simple,
            av_id,
        })
    }
}

impl<'a> EchoExtRender<'a> for BiliVideoExt<'a> {
    fn render(&self) -> impl IntoView {
        let player = match self.simple {
            true => "//bilibili.com/blackboard/html5mobileplayer.html",
            false => "//player.bilibili.com/player.html",
        };
        let mut ext = String::with_capacity(64);
        if self.simple {
            ext.push_str("&hideCoverInfo=1&danmaku=0");
        }
        match self.autoplay {
            true => ext.push_str("&autoplay=1"),
            false => ext.push_str("&autoplay=0"),
        }
        let src = format!("{}?aid={}&page=1{}", player, self.av_id, ext);
        view! {
            <div style="position: relative; width: 100%; height: 0; padding-bottom: 75%;">
              <iframe
                src=src
                style="position: absolute; width: 100%; height: 100%; left: 0; top: 0;"
              />
            </div>
        }
    }
}

#[derive(Debug, EchoExt)]
#[echo_ext(id = 3)]
pub(super) struct NetEaseMusicExt {
    id: u64,
    autoplay: bool,
}

impl<'a> EchoExtHandler<'a> for NetEaseMusicExt {
    fn extract(_: WeakArc<EchoState>, attr: &'a Ref<'a, Vec<Attribute>>) -> EchoExtResult<Self> {
        let id_str = Self::get_meta_from_attr(attr, "id")?;
        let id = id_str.parse::<u64>().map_err(|_| {
            EchoExtError::CustomValidationError(id_str.to_string(), "not a valid id")
        })?;
        let autoplay = Self::get_meta_from_attr(attr, "autoplay")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Ok(Self { id, autoplay })
    }
}

impl<'a> EchoExtRender<'a> for NetEaseMusicExt {
    fn render(&self) -> impl IntoView {
        let mut ext = String::with_capacity(8);
        match self.autoplay {
            true => ext.push_str("&auto=1"),
            false => ext.push_str("&auto=0"),
        }
        let src = format!(
            "//music.163.com/outchain/player?type=2&id={}&height=66{}",
            self.id, ext
        );
        view! {
            <iframe
                width="330"
                height="86"
                src=src
            />
        }
    }
}

#[macro_export]
macro_rules! echo_ext_dispatch {
    ($($ty:ty),+ $(,)?) => {
        pub fn validate_attr<'a>(
            id: u32,
            attr: &'a ::std::cell::Ref<'a, ::std::vec::Vec<markup5ever::Attribute>>,
        ) -> EchoExtResult<()> {
            match id {
                $(
                    < $ty as EchoExtMeta >::ID => < $ty as EchoExtHandler<'a> >::validate_attr(attr),
                )+
                _ => return Err($crate::gladiator::ext_plugins::EchoExtError::UnknownExtId(id)),
            }
        }

        pub fn render<'a>(
            id: u32,
            ctx: WeakArc<$crate::services::states::EchoState>,
            attr: &'a ::std::cell::Ref<'a, ::std::vec::Vec<markup5ever::Attribute>>,
        ) -> EchoExtResult<String> {
            match id {
                $(
                    < $ty as EchoExtMeta >::ID => Ok(< $ty as EchoExtHandler<'a> >::extract(ctx, attr)?
                            .render()
                            .to_html()),
                )+
                _ => return Err($crate::gladiator::ext_plugins::EchoExtError::UnknownExtId(id)),
            }
        }

        pub fn fuzz_hw(id: u32) -> (u32, u32) {
            match id {
                $(
                    < $ty as EchoExtMeta >::ID => (< $ty as EchoExtMeta >::FUZZ_H, < $ty as EchoExtMeta >::FUZZ_W),
                )+
                _ => (200, 300), // fallback
            }
        }

        pub const ALL_EXT_IDS: &'static [u32] = &[
            $( < $ty as EchoExtMeta >::ID, )+
        ];
    }
}

echo_ext_dispatch!(EchoResourceExt<'_>, BiliVideoExt<'_>, NetEaseMusicExt);
