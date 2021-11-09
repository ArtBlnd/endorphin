use crate::hash::*;
use crate::policy::{ExpirePolicy, Status};
use crate::storage::Storage;
use crate::{BucketIdTable, EntryId, ENTRY_TOMBSTONE};

// hashbrown internals.
use hashbrown::hash_map::DefaultHashBuilder;
use hashbrown::raw::{Bucket, RawTable};

use std::borrow::Borrow;
use std::hash::{BuildHasher, Hash};
use std::mem;

use parking_lot::RwLock;

struct HashMap<K, V, P, H = DefaultHashBuilder>
where
    P: ExpirePolicy,
{
    exp_bucket_table: BucketIdTable<(K, V, Storage<P::Storage>)>,
    exp_policy: P,
    exp_backlog: RwLock<Vec<EntryId>>,

    hash_builder: H,
    table: RawTable<(K, V, Storage<P::Storage>)>,
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
            exp_backlog: RwLock::new(Vec::new()),

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
    #[inline]
    fn grow_and_insert(
        &mut self,
        hash: u64,
        v: (K, V, Storage<P::Storage>),
    ) -> Bucket<(K, V, Storage<P::Storage>)> {
        let expired_values = self.exp_backlog.get_mut();

        let v = if !expired_values.is_empty() {
            // try remove expired items.
            for entry_id in expired_values.drain(..) {
                if entry_id == ENTRY_TOMBSTONE {
                    continue;
                }

                let bucket = self.exp_bucket_table.release_id(entry_id).unwrap();
                unsafe {
                    self.table.remove(bucket);
                }
            }

            // try insert again, if fails we need to extend inner table.
            match self.table.try_insert_no_grow(hash, v) {
                Ok(bucket) => return bucket,
                Err(v) => v,
            }
        } else {
            v
        };

        // TODO: Fork hashbrown and make internal resize method visible.
        // and replace reserve method not to reiterate over to remap entry_id and bucket.

        // seems there is no capacity left on table.
        // extend capacity and recalcalulate ids in bucket table.
        let hasher = make_hasher::<K, _, V, Storage<P::Storage>, H>(&self.hash_builder);
        self.table.reserve(self.table.capacity() / 2, hasher);

        // clear table and resize it.
        self.exp_bucket_table
            .clear_table_and_resize(self.table.capacity());
        unsafe {
            // re-init id to bucket mapping on bucket table.
            for bucket in self.table.iter() {
                let (_, _, s) = bucket.as_mut();
                self.exp_bucket_table.set_bucket(s.entry_id, bucket);
            }
        }

        // we know we have enough size to insert.
        self.table.insert_no_grow(hash, v)
    }

    #[inline]
    fn handle_status(&self, status: Status) {
        let mut expired_values = Vec::new();
        match status {
            // We do lazy expiration on get operation to keep get operation not to require mutablity.
            // also for cache friendly.
            Status::Remove(id) => expired_values.push(id),
            Status::RemoveBulk(mut id_list) => mem::swap(&mut expired_values, &mut id_list),
            Status::TryShrink => unimplemented!("shrinking table is not supported yet!"),
            // there is nothing to expire.
            Status::Alive => return,
        }

        expired_values.iter().cloned().for_each(|v| {
            let bucket = self.exp_bucket_table.get(v);
            let (_, _, s) = unsafe { bucket.as_ref() };

            // set bucket marked as expired.
            s.mark_as_removed();
            self.exp_backlog.write().push(v);
        })
    }

    #[inline]
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
        self.handle_status(self.exp_policy.on_access(s.entry_id, &mut s.storage));

        // don't give access to entry if entry expired.
        if self.exp_policy.is_expired(s.entry_id, &mut s.storage) || s.is_removed() {
            None
        } else {
            Some(v)
        }
    }

    #[inline]
    pub fn get_mut<Q: ?Sized>(&self, k: &Q) -> Option<&mut V>
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
        self.handle_status(self.exp_policy.on_access(s.entry_id, &mut s.storage));

        // don't give access to entry if entry expired.
        if self.exp_policy.is_expired(s.entry_id, &mut s.storage) || s.is_removed() {
            None
        } else {
            Some(v)
        }
    }

    #[inline]
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
                // crate a new entry storage.
                let storage = Storage::new(s, self.exp_bucket_table.acquire_id());

                match self.table.try_insert_no_grow(hash, (k, v, storage)) {
                    Ok(bucket) => bucket,
                    Err(v) => self.grow_and_insert(hash, v),
                }
            };

        let (_, _, s) = unsafe { bucket.as_mut() };
        self.exp_bucket_table.set_bucket(s.entry_id, bucket);

        self.handle_status(self.exp_policy.on_insert(s.entry_id, &mut s.storage));
        return None;
    }
}

impl<K, V, P, H> HashMap<K, V, P, H>
where
    P: ExpirePolicy,
{
    #[inline]
    pub fn clear(&mut self) {
        self.table.clear();

        self.exp_backlog.get_mut().clear();
        self.exp_policy.clear();
        self.exp_bucket_table.clear();
    }
    
    #[inline]
    pub fn len(&self) -> usize {
        self.table.len()
    }
}
