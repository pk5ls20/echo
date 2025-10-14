use moka::Expiry;
use moka::future::Cache;
use moka::notification::RemovalCause;
use paste::paste;
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant};
use time;

#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MokaExpiration(time::Duration);

impl MokaExpiration {
    pub fn new(duration: time::Duration) -> Self {
        MokaExpiration(duration)
    }
    pub fn as_duration(&self) -> Duration {
        Duration::from_secs(self.0.whole_seconds() as u64)
    }
}

pub struct EchoMokaExpiry;

impl<K, V> Expiry<K, (MokaExpiration, V)> for EchoMokaExpiry {
    fn expire_after_create(
        &self,
        _: &K,
        value: &(MokaExpiration, V),
        _: Instant,
    ) -> Option<Duration> {
        Some(value.0.as_duration())
    }
}

pub type MokaVal<V> = (MokaExpiration, V);

fn build_cache<K, V>(max_capacity: Option<u64>) -> Cache<K, MokaVal<V>>
where
    K: Clone + Eq + Hash + Send + Sync + Debug + 'static,
    V: Clone + Send + Sync + 'static,
{
    let builder = Cache::builder()
        .expire_after(EchoMokaExpiry)
        .eviction_listener(|key: Arc<K>, _value: MokaVal<V>, cause: RemovalCause| {
            tracing::trace!("Evicted key: {:?}, cause: {:?}", &*key, &cause);
        });
    let builder = if let Some(c) = max_capacity {
        builder.max_capacity(c)
    } else {
        builder
    };
    builder.build()
}

pub struct Raw;
pub struct Namespaced;

pub trait KeyStrategy<K> {
    type StoredKey;
}

impl<K> KeyStrategy<K> for Raw {
    type StoredKey = K;
}

impl<K> KeyStrategy<K> for Namespaced
where
    K: Into<String>,
{
    type StoredKey = String;
}

pub struct GroupCache<K, V, S>
where
    S: KeyStrategy<K>,
{
    inner: Cache<<S as KeyStrategy<K>>::StoredKey, MokaVal<V>>,
    _pd: PhantomData<(K, S)>,
}

impl<K, V> GroupCache<K, V, Raw>
where
    K: Clone + Eq + Hash + Send + Sync + Debug + 'static,
    V: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self::new_with_capacity(None)
    }

    pub fn new_with_capacity(max_capacity: Option<u64>) -> Self {
        Self {
            inner: build_cache::<K, V>(max_capacity),
            _pd: PhantomData,
        }
    }

    pub fn contains(&self, key: impl Into<K>) -> bool {
        let k = key.into();
        self.inner.contains_key(&k)
    }

    pub async fn get(&self, key: impl Into<K>) -> Option<MokaVal<V>> {
        let k = key.into();
        self.inner.get(&k).await
    }

    pub async fn get_with(
        &self,
        key: impl Into<K>,
        f: impl Future<Output = MokaVal<V>>,
    ) -> MokaVal<V> {
        let k = key.into();
        self.inner.get_with(k, f).await
    }

    pub async fn insert(&self, key: impl Into<K>, value: MokaVal<V>) {
        let k = key.into();
        self.inner.insert(k, value).await
    }

    pub async fn invalidate(&self, key: impl Into<K>) {
        let k = key.into();
        self.inner.invalidate(&k).await
    }

    pub async fn remove(&self, key: impl Into<K>) -> Option<V> {
        let k = key.into();
        self.inner.remove(&k).await.map(|(_, v)| v)
    }

    pub async fn run_pending_tasks(&self) {
        self.inner.run_pending_tasks().await;
    }
}

impl<K, V> GroupCache<K, V, Namespaced>
where
    K: Into<String> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self::new_with_capacity(None)
    }

    pub fn new_with_capacity(max_capacity: Option<u64>) -> Self {
        Self {
            inner: build_cache::<String, V>(max_capacity),
            _pd: PhantomData,
        }
    }

    fn ns_key(prefix: &str, key: impl Into<K>) -> String {
        let raw: String = key.into().into();
        let mut s = String::with_capacity(prefix.len() + 1 + raw.len());
        s.push_str(prefix);
        s.push(':');
        s.push_str(&raw);
        s
    }

    pub fn contains_with_prefix(&self, prefix: &'static str, key: impl Into<K>) -> bool {
        let cache_key = Self::ns_key(prefix, key);
        self.inner.contains_key(&cache_key)
    }

    pub async fn get_with_prefix(
        &self,
        prefix: &'static str,
        key: impl Into<K>,
    ) -> Option<MokaVal<V>> {
        let cache_key = Self::ns_key(prefix, key);
        self.inner.get(&cache_key).await
    }

    pub async fn get_with_with_prefix(
        &self,
        prefix: &'static str,
        key: impl Into<K>,
        f: impl Future<Output = MokaVal<V>>,
    ) -> MokaVal<V> {
        let cache_key = Self::ns_key(prefix, key);
        self.inner.get_with(cache_key, f).await
    }

    pub async fn insert_with_prefix(
        &self,
        prefix: &'static str,
        key: impl Into<K>,
        value: MokaVal<V>,
    ) {
        let cache_key = Self::ns_key(prefix, key);
        self.inner.insert(cache_key, value).await
    }

    pub async fn invalidate_with_prefix(&self, prefix: &'static str, key: impl Into<K>) {
        let cache_key = Self::ns_key(prefix, key);
        self.inner.invalidate(&cache_key).await;
    }

    pub async fn remove_with_prefix(&self, prefix: &'static str, key: impl Into<K>) -> Option<V> {
        let cache_key = Self::ns_key(prefix, key);
        self.inner.remove(&cache_key).await.map(|(_, v)| v)
    }

    pub async fn run_pending_tasks(&self) {
        self.inner.run_pending_tasks().await;
    }
}

macro_rules! define_moka_cache {
    ( $( $name:ident => {
            vis: $vis:vis,
            tk: $kty:ty,
            ty: $vty:ty,
            key_constraint: $kc:tt,
            max_size: $cap:expr $(,)?
        } )* ) => {
        paste! {
            pub struct CacheState {
                $(
                    [<inner_ $name>]: define_moka_cache!(@field_ty $kty, $vty, $kc),
                )*
            }

            impl CacheState {
                pub fn new() -> Self {
                    Self {
                        $(
                            [<inner_ $name>]:
                                define_moka_cache!(@field_new $kty, $vty, $kc, $cap),
                        )*
                    }
                }

                $(
                    define_moka_cache!(
                        @methods
                        [vis: $vis]
                        [field: [<inner_ $name>]]
                        [kty: $kty]
                        [vty: $vty]
                        [pfx: $name]
                        [$kc]
                    );
                )*
            }
        }
    };

    (@field_ty $kty:ty, $vty:ty, true)  => { GroupCache<$kty, $vty, Namespaced> };
    (@field_ty $kty:ty, $vty:ty, false) => { GroupCache<$kty, $vty, Raw> };

    (@field_new $kty:ty, $vty:ty, true,  $cap:expr) => {
        GroupCache::<$kty, $vty, Namespaced>::new_with_capacity($cap)
    };
    (@field_new $kty:ty, $vty:ty, false, $cap:expr) => {
        GroupCache::<$kty, $vty, Raw>::new_with_capacity($cap)
    };

    (@methods
        [vis: $vis:vis]
        [field: $field:ident]
        [kty: $kty:ty]
        [vty: $vty:ty]
        [pfx: $pfx:ident]
        [true]
    ) => {
        paste! {
            $vis fn [<contains_ $pfx>](&self, key: impl Into<$kty>) -> bool {
                self.$field.contains_with_prefix(stringify!($pfx), key)
            }
            $vis async fn [<get_ $pfx>](&self, key: impl Into<$kty>) -> Option<MokaVal<$vty>> {
                self.$field.get_with_prefix(stringify!($pfx), key).await
            }
            $vis async fn [<get_ $pfx _with>](
                &self,
                key: impl Into<$kty>,
                f: impl Future<Output = MokaVal<$vty>>
            ) -> MokaVal<$vty> {
                self.$field.get_with_with_prefix(stringify!($pfx), key, f).await
            }
            $vis async fn [<set_ $pfx>](&self, key: impl Into<$kty>, value: MokaVal<$vty>) {
                self.$field.insert_with_prefix(stringify!($pfx), key, value).await
            }
            $vis async fn [<invalidate_ $pfx>](&self, key: impl Into<$kty>) {
                self.$field.invalidate_with_prefix(stringify!($pfx), key).await
            }
            $vis async fn [<remove_ $pfx>](&self, key: impl Into<$kty>) -> Option<$vty> {
                self.$field.remove_with_prefix(stringify!($pfx), key).await
            }
            $vis async fn [<run_pending_ $pfx _tasks>](&self) {
                self.$field.run_pending_tasks().await;
            }
        }
    };

    (@methods
        [vis: $vis:vis]
        [field: $field:ident]
        [kty: $kty:ty]
        [vty: $vty:ty]
        [pfx: $pfx:ident]
        [false]
    ) => {
        paste! {
            $vis fn [<contains_ $pfx>](&self, key: impl Into<$kty>) -> bool {
                self.$field.contains(key)
            }
            $vis async fn [<get_ $pfx>](&self, key: impl Into<$kty>) -> Option<MokaVal<$vty>> {
                self.$field.get(key).await
            }
            $vis async fn [<get_ $pfx _with>](
                &self,
                key: impl Into<$kty>,
                f: impl Future<Output = MokaVal<$vty>>
            ) -> MokaVal<$vty> {
                self.$field.get_with(key, f).await
            }
            $vis async fn [<set_ $pfx>](&self, key: impl Into<$kty>, value: MokaVal<$vty>) {
                self.$field.insert(key, value).await
            }
            $vis async fn [<invalidate_ $pfx>](&self, key: impl Into<$kty>) {
                self.$field.invalidate(key).await
            }
            $vis async fn [<remove_ $pfx>](&self, key: impl Into<$kty>) -> Option<$vty> {
                self.$field.remove(key).await
            }
            $vis async fn [<run_pending_ $pfx _tasks>](&self) {
                self.$field.run_pending_tasks().await;
            }
        }
    };
}

define_moka_cache! {
    passkey_reg_session => {
        vis: pub,
        tk: String,
        ty: bytes::Bytes,
        key_constraint: true,
        max_size: Some(10)
    }
    passkey_auth_session => {
        vis: pub,
        tk: String,
        ty: bytes::Bytes,
        key_constraint: true,
        max_size: Some(10),
    }
    passkey_ph_session => {
        vis: pub,
        tk: i64,
        ty: Arc<crate::models::mfa::WebAuthnKV>,
        key_constraint: false,
        max_size: Some(10),
    }
    upload_tracker_session => {
        vis: pub,
        tk: uuid::Uuid,
        ty: Arc<crate::services::upload_tracker::UploadTracker>,
        key_constraint: false,
        max_size: Some(10)
    }
    res_sign => {
        vis: pub,
        tk: String,
        ty: uuid::Uuid,
        key_constraint: false,
        max_size: Some(100),
    }
    totp_flow => {
        vis: pub,
        tk: uuid::Uuid,
        ty: Arc<totp_rs::TOTP>,
        key_constraint: false,
        max_size: Some(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ValType(pub u32);

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    pub struct MyKey(String);

    impl From<MyKey> for String {
        fn from(k: MyKey) -> Self {
            k.0
        }
    }
    impl<'a> From<&'a MyKey> for String {
        fn from(k: &'a MyKey) -> Self {
            k.0.clone()
        }
    }

    define_moka_cache! {
        passkey_reg_session => {
            vis: pub,
            tk: MyKey,
            ty: Bytes,
            key_constraint: true,
            max_size: Some(10)
        }
        passkey_auth_session => {
            vis: pub,
            tk: MyKey,
            ty: Bytes,
            key_constraint: true,
            max_size: Some(10)
        }
        another_item => {
            vis: pub,
            tk: MyKey,
            ty: ValType,
            key_constraint: false,
            max_size: Some(10)
        }
    }

    #[tokio::test]
    async fn multi_prefix_same_group_should_not_collide() {
        let cache = CacheState::new();
        cache
            .set_passkey_reg_session(
                MyKey("user1".into()),
                (
                    MokaExpiration::new(time::Duration::seconds(30)),
                    Bytes::from_static(b"A"),
                ),
            )
            .await;
        cache
            .set_passkey_auth_session(
                MyKey("user1".into()),
                (
                    MokaExpiration::new(time::Duration::seconds(30)),
                    Bytes::from_static(b"B"),
                ),
            )
            .await;
        let v1 = cache
            .get_passkey_reg_session(MyKey("user1".into()))
            .await
            .unwrap();
        let v2 = cache
            .get_passkey_auth_session(MyKey("user1".into()))
            .await
            .unwrap();
        assert_ne!(v1.1, v2.1);
    }

    #[tokio::test]
    async fn mixed_groups_should_work() {
        let cache = CacheState::new();
        cache
            .set_passkey_reg_session(
                MyKey("k1".into()),
                (
                    MokaExpiration::new(time::Duration::seconds(5)),
                    Bytes::from_static(b"X"),
                ),
            )
            .await;
        cache
            .set_passkey_auth_session(
                MyKey("k1".into()),
                (
                    MokaExpiration::new(time::Duration::seconds(5)),
                    Bytes::from_static(b"Y"),
                ),
            )
            .await;
        assert_eq!(
            cache
                .get_passkey_reg_session(MyKey("k1".into()))
                .await
                .unwrap()
                .1,
            Bytes::from_static(b"X")
        );
        assert_eq!(
            cache
                .get_passkey_auth_session(MyKey("k1".into()))
                .await
                .unwrap()
                .1,
            Bytes::from_static(b"Y")
        );
        cache
            .set_another_item(
                MyKey("k2".into()),
                (MokaExpiration::new(time::Duration::seconds(5)), ValType(42)),
            )
            .await;

        let res = cache.get_another_item(MyKey("k2".into())).await.unwrap();
        assert_eq!(res.1, ValType(42));
    }

    #[tokio::test]
    async fn custom_key_type_unconstrained() {
        let cache = CacheState::new();
        let k = MyKey("ckey".into());
        cache
            .set_another_item(
                k.clone(),
                (MokaExpiration::new(time::Duration::seconds(5)), ValType(7)),
            )
            .await;
        assert!(cache.contains_another_item(k.clone()));
        let v = cache.get_another_item(k).await.unwrap();
        assert_eq!(v.1, ValType(7));
    }
}
