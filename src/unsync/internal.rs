use crate::unsync::{ Tick, ValueEntry, TTLTracer };

use hashbrown::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

pub struct UnsyncCacheInternal<K, V> {
    base: HashMap<K, ValueEntry<V>>,
    tick: Tick<65536>,

    // tracers
    ttl_tracer: TTLTracer,
}

impl<K, V> UnsyncCacheInternal<K, V>
where
    K: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            base: HashMap::new(),
            ttl_tracer: TTLTracer::new(),

            tick: Tick::new(),
        }
    }

    /// cleans all expired values and resize internal hashmap.
    pub fn cleanup_resize(&mut self, cap: usize) {
        // create new map with capaicty.
        let old_base = std::mem::replace(&mut self.base, HashMap::with_capacity(cap));
        let new_base = &mut self.base;

        // we use rayon to invalidate expired values.
        // this might slower than normal iterater when length of map is relatively short.
        new_base.extend(old_base.into_iter().filter(|(_, v)| !v.is_expired()));
        self.tick.clear();
    }

    pub fn tick(&mut self) {
        let len = self.base.len();
        let cap = self.base.capacity();

        // check we are in clean-up tick
        if self.tick.tick() {
            let expired_cnt = self.ttl_tracer.expire_trace(Instant::now()) as usize;
            if expired_cnt > len {
                return;
            }

            let cur_len = len - expired_cnt;
            if cur_len * 3 > cap {
                // we don't need resize right now. just go expire some keys.
                self.base.retain(|_, v| !v.is_expired());
                return;
            }

            // recalculate new capacity from current length of items.
            let new_cap = cur_len * 2;
            self.cleanup_resize(new_cap);

            return;
        }

        // hash table is full
        // we need to remove all expired values and make bigger one.
        if len + 1 >= cap {
            self.ttl_tracer.expire_trace(Instant::now()) as usize;

            // resize capacity by double.
            let new_cap = cap * 2;
            self.cleanup_resize(new_cap);

            return;
        }
    }

    pub fn clear(&mut self) {
        self.base.clear();
        self.tick.clear();
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let result = if let Some(entry) = self.base.get_mut(&key) {
            entry.take_inner()
        } else {
            None
        };

        return result;
    }

    pub fn insert(&mut self, key: K, val: V, ttl: Duration) -> Option<V> {
        // compute expiration date.
        let exp_at = Instant::now() + ttl;

        // add value into entry.
        let val = ValueEntry::new(val, exp_at.clone());
        let result = if let Some(entry) = self.base.insert(key, val) {
            entry.into_inner()
        } else {
            None
        };

        self.ttl_tracer.trace_ttl(exp_at);
        return result;
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        if let Some(entry) = self.base.get(&key) {
            return entry.get();
        }

        return None;
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if let Some(entry) = self.base.get_mut(&key) {
            return entry.get_mut();
        }

        return None;
    }
}
