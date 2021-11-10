use std::borrow::Borrow;
use std::hash::{BuildHasher, Hash};

#[cfg_attr(feature = "inline-more", inline)]
pub(crate) fn make_hasher<K, Q, V, S, H>(
    hash_builder: &H,
) -> impl Fn(&(Q, V, S)) -> u64 + '_ + Clone
where
    K: Borrow<Q>,
    Q: Hash,
    H: BuildHasher,
{
    move |val| make_hash::<K, Q, H>(hash_builder, &val.0)
}

#[cfg_attr(feature = "inline-more", inline)]
pub(crate) fn equivalent_key<Q, K, V, S>(k: &Q) -> impl Fn(&(K, V, S)) -> bool + '_ + Clone
where
    K: Borrow<Q>,
    Q: ?Sized + Eq,
{
    move |x| k.eq(x.0.borrow())
}

#[cfg_attr(feature = "inline-more", inline)]
pub(crate) fn equivalent<Q, K>(k: &Q) -> impl Fn(&K) -> bool + '_ + Clone
where
    K: Borrow<Q>,
    Q: ?Sized + Eq,
{
    move |x| k.eq(x.borrow())
}

#[cfg_attr(feature = "inline-more", inline)]
pub(crate) fn make_hash<K, Q, H>(hash_builder: &H, val: &Q) -> u64
where
    K: Borrow<Q>,
    Q: Hash + ?Sized,
    H: BuildHasher,
{
    use core::hash::Hasher;
    let mut state = hash_builder.build_hasher();
    val.hash(&mut state);
    state.finish()
}

#[cfg_attr(feature = "inline-more", inline)]
pub(crate) fn make_insert_hash<K, H>(hash_builder: &H, val: &K) -> u64
where
    K: Hash,
    H: BuildHasher,
{
    use core::hash::Hasher;
    let mut state = hash_builder.build_hasher();
    val.hash(&mut state);
    state.finish()
}
