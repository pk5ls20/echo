use crate::utils::smart_to_string::prelude::*;
use ahash::HashMap;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynSettingsValue<'a> {
    pub val: String,
    #[serde(borrow)]
    pub description: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub side_effects: Option<Cow<'a, str>>,
}

pub struct DynSettingsBindValue<'a, T>
where
    T: SmartString,
{
    pub val: T,
    pub description: Option<Cow<'a, str>>,
    pub side_effects: Option<Cow<'a, str>>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DynSettingsValueRow {
    pub val: String,
}

pub struct DynSettingsValueBindRow<T>
where
    T: SmartString,
{
    pub val: T,
}

pub type DynSettingsKvMap<'a> = HashMap<&'a str, DynSettingsValue<'a>>;

pub trait DynSetting {
    type Value: SmartString;
    fn key(&self) -> &'static str;
    const DESC: Option<&'static str>;
    const SIDE_EFFECTS: Option<&'static str>;
    fn default_val() -> Self::Value;
    fn parse(&self, s: &str) -> SmartStringResult<Self::Value>;
    fn render(&self, v: &Self::Value) -> SmartStringResult<String>;
}

macro_rules! opt {
    () => {
        None
    };
    ($s:expr) => {
        Some($s)
    };
}

macro_rules! define_dyn_settings {
    (
        $(
            $namespace:ident => {
                $(
                    $variant:ident => {
                        typ: $ty:path,
                        default_val: $default_val:expr
                        $(, desc: $desc:expr)?
                        $(, side_effects: $side_effects:expr)?
                    }
                ),* $(,)?
            }
        ),* $(,)?
    ) => {
        use ahash::HashMapExt;
        use once_cell::sync::Lazy;
        use crate::smart_string;
        use crate::utils::smart_to_string::ParseTarget;

        $(
            $(
                pub struct $variant;

                impl DynSetting for $variant {
                    type Value = $ty;

                    fn key(&self) -> &'static str {
                        concat!(stringify!($namespace), ".", stringify!($variant))
                    }

                    const DESC: Option<&'static str> = opt!($($desc)?);

                    const SIDE_EFFECTS: Option<&'static str> = opt!($($side_effects)?);

                    fn default_val() -> Self::Value {
                        $default_val
                    }

                    fn parse(&self, s: &str) -> SmartStringResult<Self::Value> {
                        ParseTarget::<Self::Value>::NEW.smart_parse(s)
                    }

                    fn render(&self, v: &Self::Value) -> SmartStringResult<String> {
                        (&*v).smart_to_string()
                    }
                }
            )*
        )*

        #[derive(Debug)]
        pub enum DynSettingCollector {
            $( $( $variant, )* )*
        }

        impl DynSettingCollector {
            pub fn original_kv_map() -> &'static DynSettingsKvMap<'static> {
                static KV: Lazy<DynSettingsKvMap> = Lazy::new(|| {
                    let mut m = HashMap::with_capacity(0 $( $( + (stringify!($variant), 1usize).1 )* )*);
                    $(
                        $(
                            {
                                let key = $variant.key();
                                let val = DynSettingsValue {
                                    val: smart_string!($variant::default_val()).unwrap(),
                                    description: $variant::DESC.map(std::borrow::Cow::Borrowed),
                                    side_effects: $variant::SIDE_EFFECTS.map(std::borrow::Cow::Borrowed),
                                };
                                m.insert(key, val);
                            }
                        )*
                    )*
                    m
                });
                &*KV
            }

            pub fn try_parse<'a>(
                key: &str,
                input: &str,
            ) -> Option<SmartStringResult<DynSettingsValue<'a>>> {
                match key {
                    $(
                        $(
                            concat!(stringify!($namespace), ".", stringify!($variant)) => {
                                let s = $variant;
                                Some(match s.parse(input) {
                                    Ok(v) => s.render(&v).map(|rendered| DynSettingsValue {
                                        val: rendered,
                                        description: $variant::DESC.map(std::borrow::Cow::Borrowed),
                                        side_effects: $variant::SIDE_EFFECTS.map(std::borrow::Cow::Borrowed),
                                    }),
                                    Err(e) => Err(e),
                                })
                            },
                        )*
                    )*
                    _ => None,
                }
            }
        }
    };
}

define_dyn_settings! {
    Site => {
        AllowRegister => {
            typ: bool,
            default_val: true,
            desc: "Whether open registration is permitted"
        },
        RegisterNeedInvitationCode => {
            typ: bool,
            default_val: true,
            desc: "Whether registration requires invitation"
        },
    },
    WebAuthn => {
        RpId => {
            typ: String,
            default_val: "localhost".to_string(),
            desc: "The relying on party id for WebAuthn, usually your domain name",
            side_effects: "Changing this will invalidate existing WebAuthn credentials!"
        },
        RpOrigin => {
            typ: String,
            default_val: "http://localhost:8080".to_string(),
            desc: "The origin URL for the WebAuthn relying on party",
            side_effects: "Changing this will invalidate existing WebAuthn credentials!"
        },
        RpName => {
            typ: String,
            default_val: "Echo".to_string(),
            desc: "The name of the relying on party"
        },
    },
    Upload => {
        MaxFileSize => {
            typ: u64,
            default_val: 10 * 1024 * 1024, // 10 MB
            desc: "Maximum allowed file size for uploads in bytes"
        },
        UploadChunkSize => {
            typ: std::num::NonZeroU32,
            default_val: std::num::NonZeroU32::new(512 * 1024).unwrap(), // 512 KB
            desc: "The chunk size in bytes for each upload operation in bytes, must be non-zero"
        },
        AllowMimeTypes => {
            typ: Option<Vec<Cow<'static, str>>>,
            default_val: Some(
                vec![
                    "image/jpeg",
                    "image/png",
                    "image/gif",
                    "image/webp",
                    "image/tiff",
                    "image/bmp",
                    "image/heif",
                    "image/avif",
                ]
                .into_iter()
                .map(|it| it.into())
                .collect()
            ),
            desc: "List of allowed MIME types for uploads"
        },
    }
}
