mod internal;
pub(crate) use internal::*;
mod value_entry;
pub(crate) use value_entry::*;
mod tick;
pub(crate) use tick::*;
mod ttl_tracer;
pub(crate) use ttl_tracer::*;

use std::cell::UnsafeCell;
use std::hash::Hash;
use std::marker::PhantomData;
use std::time::Duration;

pub struct UnsyncCache<K, V>
where
    K: Hash + Eq,
{
    __p: PhantomData<*const ()>,
    base: UnsafeCell<UnsyncCacheInternal<K, V>>,
}

impl<K, V> UnsyncCache<K, V>
where
    K: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            __p: Default::default(),
            base: UnsafeCell::new(UnsyncCacheInternal::new()),
        }
    }

    fn tick(&self) {
        unsafe { &mut *self.base.get() }.tick();
    }

    fn base(&self) -> &UnsyncCacheInternal<K, V> {
        unsafe { &*self.base.get() }
    }

    fn base_mut(&mut self) -> &mut UnsyncCacheInternal<K, V> {
        unsafe { &mut *self.base.get() }
    }

    pub fn clear(&mut self) {
        self.tick();
        self.base_mut().clear();
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.tick();
        self.base_mut().remove(key)
    }

    pub fn insert(&mut self, key: K, val: V, ttl: Duration) -> Option<V> {
        self.tick();
        self.base_mut().insert(key, val, ttl)
    }

    pub fn get(&self, key: &K) -> Option<&'_ V> {
        self.tick();
        self.base().get(key)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.tick();
        self.base_mut().get_mut(key)
    }
}


#[cfg(test)]
mod test {
    use super::*;

    use std::time::Instant;
    use std::collections::HashMap;

    #[test]
    pub fn test1() {
        let mut cache = UnsyncCache::new();

        let now1 = Instant::now();
        for i in 0..10000000 {
            cache.insert(i, "hello!".to_string(), Duration::from_millis(100));
        }
        let now2 = Instant::now();

        println!("{:?}", now2 - now1);
    }

    #[test]
    pub fn test2() {
        let mut cache = HashMap::new();

        let now1 = Instant::now();
        for i in 0..10000000 {
            cache.insert(i, "hello!".to_string());
        }
        let now2 = Instant::now();

        println!("{:?}", now2 - now1);
    }
}