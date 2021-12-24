use crate::{policy::ExpirePolicy, HashMap};

use hashbrown::hash_map::DefaultHashBuilder;

use std::{
    borrow::Borrow,
    hash::{BuildHasher, Hash},
};

pub struct HashSet<T, P, H>
where
    P: ExpirePolicy,
{
    map: HashMap<T, (), P, H>,
}

impl<T, P> HashSet<T, P, DefaultHashBuilder>
where
    P: ExpirePolicy,
{
    pub fn new(policy: P) -> Self {
        Self {
            map: HashMap::new(policy),
        }
    }

    pub fn with_capacity(capacity: usize, policy: P) -> Self {
        Self {
            map: HashMap::with_capacity(capacity, policy),
        }
    }
}

impl<T, P, H> HashSet<T, P, H>
where
    P: ExpirePolicy,
    T: Hash + Eq,
    H: BuildHasher,
{
    #[inline]
    pub fn get<Q: ?Sized>(&self, value: &Q) -> Option<&()>
    where
        T: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.map.get(value)
    }
}
