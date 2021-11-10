use crate::hash::*;
use crate::instrinsic::unlikely;
use crate::policy::{Command, ExpirePolicy};
use crate::storage::Storage;
use crate::{EntryId, EntryIdTable};

// hashbrown internals.
use hashbrown::hash_map::DefaultHashBuilder;
use hashbrown::hash_map::HashMap as H;
use hashbrown::raw::{Bucket, RawIter, RawTable};

use std::borrow::Borrow;
use std::hash::{BuildHasher, Hash};
use std::iter::Iterator;
use std::marker::PhantomData;
use std::mem;

use crossbeam::queue::SegQueue;

pub struct HashMap<K, V, P, H = DefaultHashBuilder>
where
    P: ExpirePolicy,
{
    exp_bucket_table: EntryIdTable<(K, V, Storage<P::Storage>)>,
    exp_policy: P,
    exp_backlog: SegQueue<Vec<Option<EntryId>>>,

    hash_builder: H,
    table: RawTable<(K, V, Storage<P::Storage>)>,
}

unsafe impl<K, V, P, H> Sync for HashMap<K, V, P, H>
where
    P: ExpirePolicy,
    K: Sync,
    V: Sync,
    P: Sync,
    H: Sync,
{
}

unsafe impl<K, V, P, H> Send for HashMap<K, V, P, H>
where
    P: ExpirePolicy,
    K: Send,
    V: Send,
    P: Send,
    H: Send,
{
}

impl<K, V, P> HashMap<K, V, P, DefaultHashBuilder>
where
    P: ExpirePolicy,
{
    pub fn new(policy: P) -> Self {
        let table = RawTable::new();

        Self {
            exp_bucket_table: EntryIdTable::new(),
            exp_policy: policy,
            exp_backlog: SegQueue::new(),

            hash_builder: DefaultHashBuilder::default(),
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
    fn process_single_backlog(&mut self) {
        if self.exp_backlog.is_empty() {
            return;
        }

        if let Some(backlog) = self.exp_backlog.pop() {
            for entry_id in backlog.into_iter().filter_map(|v| v) {
                // bucket could already removed and its empty storage.
                if let Some(bucket) = self.exp_bucket_table.release_slot(entry_id) {
                    unsafe {
                        self.table.remove(bucket);
                    }
                }
            }
        }
    }

    #[inline]
    fn grow_and_insert(
        &mut self,
        hash: u64,
        v: (K, V, Storage<P::Storage>),
    ) -> Bucket<(K, V, Storage<P::Storage>)> {
        let mut has_backlog = false;
        while let Some(backlog) = self.exp_backlog.pop() {
            for entry_id in backlog.into_iter().filter_map(|v| v) {
                // bucket could already removed and its empty storage.
                if let Some(bucket) = self.exp_bucket_table.release_slot(entry_id) {
                    unsafe {
                        self.table.remove(bucket);
                    }
                }
            }

            has_backlog |= true;
        }

        // try insert again, if fails we need to extend inner table.
        let v = if has_backlog {
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
        self.reserve((self.table.capacity() + 1) * 3 / 2);

        // we know we have enough size to insert.
        self.table.insert_no_grow(hash, v)
    }

    #[inline]
    fn handle_status(&self, status: Command) {
        let mut removed = Vec::new();
        match status {
            // We do lazy expiration on get operation to keep get operation not to require mutablity.
            // also for cache friendly.
            Command::Remove(id) => removed.push(Some(id)),
            Command::RemoveBulk(mut id_list) => mem::swap(&mut removed, &mut id_list),
            Command::TryShrink => unimplemented!("shrinking table is not supported yet!"),
            // there is nothing to expire.
            Command::Noop => return,
        }

        removed.iter().cloned().filter_map(|v| v).for_each(|v| {
            if let Some(bucket) = self.exp_bucket_table.get(v) {
                let (_, _, s) = unsafe { bucket.as_ref() };

                // set bucket marked as expired.
                s.mark_as_removed();
            }
        });

        self.exp_backlog.push(removed);
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
        if self.exp_policy.is_expired(s.entry_id, &mut s.storage) || unlikely(s.is_removed()) {
            None
        } else {
            Some(v)
        }
    }

    #[inline]
    pub fn get_key_value<Q: ?Sized>(&self, k: &Q) -> Option<(&K, &V)>
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
        let (k, v, s) = unsafe { bucket.as_mut() };
        self.handle_status(self.exp_policy.on_access(s.entry_id, &mut s.storage));

        // don't give access to entry if entry expired.
        if self.exp_policy.is_expired(s.entry_id, &mut s.storage) || unlikely(s.is_removed()) {
            None
        } else {
            Some((k, v))
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
        if self.exp_policy.is_expired(s.entry_id, &mut s.storage) || unlikely(s.is_removed()) {
            None
        } else {
            Some(v)
        }
    }

    #[inline]
    pub fn contains_key<Q: ?Sized>(&self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.get(k).is_some()
    }

    #[inline]
    pub fn insert(&mut self, k: K, v: V, init: P::Info) -> Option<V> {
        // try to process single backlog when on every mutable state.
        self.process_single_backlog();

        let hash = make_insert_hash::<K, H>(&self.hash_builder, &k);

        // initialize storage with info value.
        let s = self.exp_policy.init_storage(init);
        let storage = Storage::new(s, self.exp_bucket_table.acquire_slot());

        let (bucket, old_v) =
            if let Some((_, old_v, old_s)) = self.table.get_mut(hash, equivalent_key(&k)) {
                let bucket = self.exp_bucket_table.get(old_s.entry_id).unwrap();

                // we need to mark it as removed. but not releasing id.
                // and assign a new id to track it.
                self.exp_bucket_table.set_bucket(old_s.entry_id, None);
                self.exp_bucket_table
                    .set_bucket(storage.entry_id, Some(bucket.clone()));

                *old_s = storage;
                (bucket, Some(mem::replace(old_v, v)))
            } else {
                let bucket = match self.table.try_insert_no_grow(hash, (k, v, storage)) {
                    Ok(bucket) => bucket,
                    Err(v) => self.grow_and_insert(hash, v),
                };

                (bucket, None)
            };

        let (_, _, s) = unsafe { bucket.as_mut() };
        self.exp_bucket_table.set_bucket(s.entry_id, Some(bucket));

        self.handle_status(self.exp_policy.on_insert(s.entry_id, &mut s.storage));
        return old_v;
    }

    #[inline]
    pub fn remove<Q: ?Sized>(&mut self, k: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        // try to process single backlog when on every mutable state.
        self.process_single_backlog();

        // Avoid `Option::map` because it bloats LLVM IR.
        let hash = make_hash::<K, Q, H>(&self.hash_builder, &k);
        let entry = match self.table.remove_entry(hash, equivalent_key(k)) {
            Some((_, v, s)) => {
                self.exp_bucket_table.set_bucket(s.entry_id, None);
                Some(v)
            }
            None => None,
        };

        return entry;
    }

    #[inline]
    pub fn remove_entry<Q: ?Sized>(&mut self, k: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        // try to process single backlog when on every mutable state.
        self.process_single_backlog();

        // Avoid `Option::map` because it bloats LLVM IR.
        let hash = make_hash::<K, Q, H>(&self.hash_builder, &k);
        let entry = match self.table.remove_entry(hash, equivalent_key(k)) {
            Some((k, v, s)) => {
                self.exp_bucket_table.set_bucket(s.entry_id, None);
                Some((k, v))
            }
            None => None,
        };

        return entry;
    }

    #[inline]
    fn update_bucket_id(&mut self) {
        unsafe {
            // update id to bucket mapping on bucket table.
            for bucket in self.table.iter() {
                let (_, _, s) = bucket.as_mut();
                self.exp_bucket_table.set_bucket(s.entry_id, Some(bucket));
            }
        }
    }

    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        let hasher = make_hasher::<K, _, V, Storage<P::Storage>, H>(&self.hash_builder);
        self.table.reserve(additional, hasher);
        self.update_bucket_id();
    }

    #[inline]
    pub fn shrink_to_fit(&mut self) {
        let hasher = make_hasher::<K, _, V, Storage<P::Storage>, H>(&self.hash_builder);
        self.table.shrink_to(0, hasher);
        self.update_bucket_id();
    }

    #[inline]
    pub fn shrink_to(&mut self, min_capacity: usize) {
        let hasher = make_hasher::<K, _, V, Storage<P::Storage>, H>(&self.hash_builder);
        self.table.shrink_to(min_capacity, hasher);
        self.update_bucket_id();
    }
}

impl<K, V, P, H> HashMap<K, V, P, H>
where
    P: ExpirePolicy,
{
    #[inline]
    pub fn clear(&mut self) {
        self.table.clear();

        self.exp_backlog = SegQueue::new();
        self.exp_policy.clear();
        self.exp_bucket_table.clear();
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.table.len()
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.table.capacity()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn iter(&self) -> Iter<'_, K, V, Storage<P::Storage>> {
        unsafe {
            Iter {
                inner: self.table.iter(),
                marker: PhantomData,
            }
        }
    }

    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V, Storage<P::Storage>> {
        unsafe {
            IterMut {
                inner: self.table.iter(),
                marker: PhantomData,
            }
        }
    }
}

#[derive(Clone)]
pub struct Iter<'a, K, V, P> {
    inner: RawIter<(K, V, P)>,
    marker: PhantomData<(&'a K, &'a V, &'a P)>,
}

impl<'a, K, V, P> Iterator for Iter<'a, K, V, P> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|v| unsafe { v.as_ref() })
            .map(|(k, v, _)| (k, v))
    }
}

#[derive(Clone)]
pub struct IterMut<'a, K, V, P> {
    inner: RawIter<(K, V, P)>,
    marker: PhantomData<(&'a K, &'a V, &'a P)>,
}

impl<'a, K, V, P> Iterator for IterMut<'a, K, V, P> {
    type Item = (&'a mut K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|v| unsafe { v.as_mut() })
            .map(|(k, v, _)| (k, v))
    }
}
