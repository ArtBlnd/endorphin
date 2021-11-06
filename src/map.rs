use crate::hash::*;
use crate::storage::Storage;
use crate::{BucketIdTable, EntryId, ExpirePolicy, Status};

// hashbrown internals.
use hashbrown::hash_map::DefaultHashBuilder;
use hashbrown::raw::{Bucket, RawTable};
use hashbrown::HashMap as HashHash;

use std::borrow::Borrow;
use std::hash::{BuildHasher, Hash};
use std::mem;

use parking_lot::RwLock;

struct HashMap<K, V, P, H = DefaultHashBuilder>
where
    P: ExpirePolicy,
{
    pub(crate) exp_bucket_table: BucketIdTable<(K, V, Storage<P::Storage>)>,
    pub(crate) exp_policy: P,
    pub(crate) exp_values: RwLock<Vec<EntryId>>,

    pub(crate) hash_builder: H,
    pub(crate) table: RawTable<(K, V, Storage<P::Storage>)>,
}

impl<K, V, P, H> HashMap<K, V, P, H>
where
    P: ExpirePolicy,
    H: Default,
{
    pub fn new(policy: P) -> Self {
        let table = RawTable::new();

        Self {
            exp_bucket_table: BucketIdTable::with_capacity(table.capacity()),
            exp_policy: policy,
            exp_values: RwLock::new(Vec::new()),

            hash_builder: H::default(),
            table,
        }
    }
}

impl<K, V, P, H> HashMap<K, V, P, H>
where
    P: ExpirePolicy,
    K: Hash + Eq,

    H: BuildHasher,
{
    fn grow_and_insert(
        &mut self,
        hash: u64,
        v: (K, V, Storage<P::Storage>),
    ) -> Bucket<(K, V, Storage<P::Storage>)> {
        let expired_values = self.exp_values.get_mut();

        let v = if !expired_values.is_empty() {
            // try insert something if some values has been expired.
            for entry_id in expired_values.drain(..) {
                let bucket = self.exp_bucket_table.release_id(entry_id).unwrap();
                unsafe {
                    self.table.remove(bucket);
                }
            }

            match self.table.try_insert_no_grow(hash, v) {
                Ok(bucket) => return bucket,
                Err(v) => v,
            }
        } else {
            v
        };

        // seems there is no capacity left on table.
        // extend capacity and recalcalulate id to bucket table.
        

        todo!();
    }

    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        let hash = make_hash::<K, Q, H>(&self.hash_builder, &k);
        let bucket = match self.table.find(hash, equivalent_key(k)) {
            Some(bucket) => bucket,
            None => return None,
        };

        // fire on_insert event.
        let (_, v, s) = unsafe { bucket.as_mut() };
        match self.exp_policy.on_access(s.entry_id, &mut s.storage) {
            // We do lazy expiration on get operation to keep get operation not to require mutablity.
            // also for cache friendly.
            Status::Expired(id) => {
                self.exp_values.write().push(id);
            }
            Status::ExpiredVec(id_list) => {
                self.exp_values.write().extend_from_slice(&id_list);
            }
            Status::Alive => {}
        }

        return Some(v);
    }

    pub fn insert(&mut self, k: K, v: V, init: P::Info) -> Option<V> {
        // initialize storage with info value.
        let s = self.exp_policy.init_storage(init);

        // insert values into internal table.
        let hash = make_insert_hash::<K, H>(&self.hash_builder, &k);
        let bucket =
            if let Some((_, inner_v, inner_s)) = self.table.get_mut(hash, equivalent_key(&k)) {
                inner_s.storage = s;
                return Some(mem::replace(inner_v, v));
            } else {
                let storage = Storage {
                    storage: s,
                    entry_id: self.exp_bucket_table.acquire_id(),
                };

                match self.table.try_insert_no_grow(hash, (k, v, storage)) {
                    Ok(bucket) => bucket,
                    Err(v) => self.grow_and_insert(hash, v),
                }
            };

        let (_, _, s) = unsafe { bucket.as_mut() };
        self.exp_bucket_table.register_bucket(s.entry_id, bucket);

        // fire on_insert event.
        match self.exp_policy.on_insert(s.entry_id, &mut s.storage) {
            Status::Expired(id) => {
                let bucket = self.exp_bucket_table.release_id(id).unwrap();
                unsafe { self.table.erase(bucket) };
            }
            Status::ExpiredVec(id_list) => {
                id_list.into_iter().for_each(|id| {
                    let bucket = self.exp_bucket_table.release_id(id).unwrap();
                    unsafe { self.table.erase(bucket) };
                });
            }
            Status::Alive => {}
        }

        return None;
    }
}
