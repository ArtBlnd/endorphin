use crate::hash::*;
use crate::instrinsic::unlikely;
use crate::policy::{Command, ExpirePolicy};
use crate::storage::Storage;
use crate::{EntryId, EntryIdTable};

// hashbrown internals.
use hashbrown::hash_map::DefaultHashBuilder;
use hashbrown::raw::{Bucket, RawDrain, RawIter, RawTable};

use std::borrow::Borrow;
use std::hash::{BuildHasher, Hash};
use std::iter::Iterator;
use std::marker::PhantomData;
use std::mem;
use std::panic::{RefUnwindSafe, UnwindSafe};

use crossbeam::queue::SegQueue;

/// A hash map implemented with [`hashbrown`] internal.
///
/// `HashMap` supports both [Standard-like] and [hashbrown-like] interfaces.
///
/// [`hashbrown`]: https://docs.rs/hashbrown/latest/hashbrown/index.html
/// [Standard-like]: https://doc.rust-lang.org/std/collections/struct.HashMap.html
/// [hashbrown-like]: https://docs.rs/hashbrown/latest/hashbrown/struct.HashMap.html
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

impl<K, V, P, H> Unpin for HashMap<K, V, P, H>
where
    P: ExpirePolicy,
    K: Unpin,
    V: Unpin,
    P: Unpin,
    H: Unpin,
{
}

impl<K, V, P, H> UnwindSafe for HashMap<K, V, P, H>
where
    P: ExpirePolicy,
    K: UnwindSafe,
    V: UnwindSafe,
    P: UnwindSafe,
    H: UnwindSafe,
{
}

impl<K, V, P, H> RefUnwindSafe for HashMap<K, V, P, H>
where
    P: ExpirePolicy,
    K: RefUnwindSafe,
    V: RefUnwindSafe,
    P: RefUnwindSafe,
    H: RefUnwindSafe,
{
}

impl<K, V, P> HashMap<K, V, P, DefaultHashBuilder>
where
    P: ExpirePolicy,
{
    /// Creates an empty `HashMap` with specified expire policy.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::<u32, u32, _>::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    /// ```
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

    pub fn with_capacity(capacity: usize, policy: P) -> Self {
        let table = RawTable::with_capacity(capacity);

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

    unsafe fn raw_insert(
        &mut self,
        k: K,
        v: V,
        init: P::Info,
    ) -> (Bucket<(K, V, Storage<P::Storage>)>, Option<V>) {
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

                let old_s = mem::replace(old_s, storage);
                let old_v = mem::replace(old_v, v);
                if self.exp_policy.is_expired(old_s.entry_id, &old_s.storage)
                    || unlikely(old_s.is_removed())
                {
                    (bucket, None)
                } else {
                    (bucket, Some(old_v))
                }
            } else {
                let bucket = match self.table.try_insert_no_grow(hash, (k, v, storage)) {
                    Ok(bucket) => bucket,
                    Err(v) => self.grow_and_insert(hash, v),
                };

                (bucket, None)
            };

        let s = &mut bucket.as_mut().2;

        self.exp_bucket_table
            .set_bucket(s.entry_id, Some(bucket.clone()));

        self.handle_status(self.exp_policy.on_insert(s.entry_id, &mut s.storage));

        (bucket, old_v)
    }

    /// Returns a reference to the value corresponding to the key.
    ///
    /// If the entry is expired, returns `None`
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "0", ());
    /// assert_eq!(cache.get(&0), Some(&"0"));
    /// assert_eq!(cache.get(&1), None);
    ///
    /// sleep(Duration::from_millis(10));
    /// assert_eq!(cache.get(&0), None);
    /// ```
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

    /// Returns the key-value pair corresponding to the supplied key.
    ///
    /// If the entry is expired, returns `None`
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "0", ());
    ///
    /// assert_eq!(cache.get_key_value(&0), Some((&0, &"0")));
    /// assert_eq!(cache.get_key_value(&1), None);
    ///
    /// sleep(Duration::from_millis(10));
    /// assert_eq!(cache.get_key_value(&0), None);
    /// ```
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

    /// Returns a mutable reference to the value corresponding to the key.
    ///
    /// If the entry is expired, returns `None`
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "0", ());
    /// if let Some(x) = cache.get_mut(&0) {
    ///     *x = "1";
    ///
    ///     sleep(Duration::from_millis(10));
    ///     assert_eq!(*x, "1");
    /// }
    /// assert!(cache.get(&0).is_none());
    /// ```
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

    /// Returns `true` if the `HashMap` contains a value for the specified key.
    ///
    /// If the entry is expired, returns `false`
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "0", ());
    /// assert!(cache.contains_key(&0));
    /// sleep(Duration::from_millis(10));
    /// assert!(!cache.contains_key(&0));
    /// ```
    #[inline]
    pub fn contains_key<Q: ?Sized>(&self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.get(k).is_some()
    }

    /// Inserts a key-value pair into the `HashMap`.
    ///
    /// If the `HashMap` did not have this key present, None is returned.
    ///
    /// If the `HashMap` did have this key present, the value is updated, and the old value is returned.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// assert_eq!(cache.insert(0, "a", ()), None);
    /// assert!(!cache.is_empty());
    ///
    /// assert_eq!(cache.insert(0, "b", ()), Some("a"));
    /// sleep(Duration::from_millis(10));
    /// assert_eq!(cache.insert(0, "c", ()), None);
    /// ```
    #[inline]
    pub fn insert(&mut self, k: K, v: V, init: P::Info) -> Option<V> {
        let (_, old_v) = unsafe { self.raw_insert(k, v, init) };

        return old_v;
    }

    /// Removes a key from the `HashMap`, returning the value at the key if the key was previously in the `HashMap`.
    ///
    /// If the `HashMap` did not have this key present, None is returned.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "a", ());
    /// cache.insert(1, "b", ());
    ///
    /// assert_eq!(cache.remove(&1), Some("b"));
    ///
    /// sleep(Duration::from_millis(15));
    /// assert_eq!(cache.remove(&0), None);
    /// ```
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

                if self.exp_policy.is_expired(s.entry_id, &s.storage) || unlikely(s.is_removed()) {
                    None
                } else {
                    Some(v)
                }
            }
            None => None,
        };

        return entry;
    }

    /// Removes a key from the `HashMap`, returning the stored key and value if the key was previously in the `HashMap`.
    ///
    /// If the `HashMap` did not have this key present, None is returned.
    ///
    /// # Example
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "a", ());
    /// cache.insert(1, "b", ());
    ///
    /// assert_eq!(cache.remove_entry(&1), Some((1, "b")));
    ///
    /// sleep(Duration::from_millis(15));
    /// assert_eq!(cache.remove_entry(&0), None);
    /// ```
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

                if self.exp_policy.is_expired(s.entry_id, &s.storage) || unlikely(s.is_removed()) {
                    None
                } else {
                    Some((k, v))
                }
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

    /// Reserves capacity for at least `additional` more elements to be inserted in the `HashMap`. The collection may reserve more space to avoid frequent reallocations.
    ///
    /// This function updates internal bucket id which tracks [`EntryId`] because of bucket relocation.
    ///
    /// # Panics
    ///
    /// Panics if the new allocation size overflows usize or out of memory.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::<u32, u32, LazyFixedTTLPolicy>::new(LazyFixedTTLPolicy::new(
    ///     Duration::from_secs(30),
    /// ));
    /// cache.reserve(10);
    /// ```
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        let hasher = make_hasher::<K, _, V, Storage<P::Storage>, H>(&self.hash_builder);
        self.table.try_reserve(additional, hasher).unwrap();
        self.update_bucket_id();
    }

    /// Shrinks the capacity of the `HashMap` as much as possible. It will drop down as much as possible while maintaining the internal rules and possibly leaving some space in accordance with the resize policy.
    ///
    /// This function updates internal bucket id which tracks [`EntryId`] because of bucket relocation.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::with_capacity(20, LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "a", ());
    /// cache.insert(1, "b", ());
    ///
    /// assert!(cache.capacity() >= 20);
    /// cache.shrink_to_fit();
    /// assert!(cache.capacity() >= 2);
    /// ```
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        let hasher = make_hasher::<K, _, V, Storage<P::Storage>, H>(&self.hash_builder);
        self.table.shrink_to(0, hasher);
        self.update_bucket_id();
    }

    /// Shrinks the capacity of the `HashMap` with a lower limit. It will drop down no lower than the supplied limit while maintaining the internal rules and possibly leaving some space in accordance with the resize policy.
    ///
    /// This function updates internal bucket id which tracks [`EntryId`] because of bucket relocation.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::with_capacity(20, LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "a", ());
    /// cache.insert(1, "b", ());
    /// assert!(cache.capacity() >= 20);
    ///
    /// cache.shrink_to(10);
    /// assert!(cache.capacity() >= 10);
    ///
    /// cache.shrink_to(0);
    /// assert!(cache.capacity() >= 2);
    /// ```
    #[inline]
    pub fn shrink_to(&mut self, min_capacity: usize) {
        let hasher = make_hasher::<K, _, V, Storage<P::Storage>, H>(&self.hash_builder);
        self.table.shrink_to(min_capacity, hasher);
        self.update_bucket_id();
    }

    /// Gets the given key’s corresponding entry in the `HashMap` for in-place manipulation.
    ///
    /// # Example
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut letters = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_secs(10)));
    ///
    /// for ch in "an easy-to-use cache library".chars() {
    ///     let counter = letters.entry(ch).or_insert(0, ());
    ///     *counter += 1;
    /// }
    ///
    /// assert_eq!(letters.get(&'a'), Some(&4));
    /// assert_eq!(letters.get(&'r'), Some(&2));
    /// assert_eq!(letters.get(&'t'), Some(&1));
    /// assert_eq!(letters.get(&'b'), Some(&1));
    /// assert_eq!(letters.get(&'l'), Some(&1));
    /// assert_eq!(letters.get(&'n'), Some(&1));
    /// assert_eq!(letters.get(&'d'), None);
    /// ```
    pub fn entry(&mut self, key: K) -> Entry<'_, K, V, P, H> {
        let hash = make_insert_hash(&self.hash_builder, &key);

        if let Some(elem) = self.table.find(hash, equivalent_key(&key)) {
            let s = unsafe { &elem.as_ref().2 };
            if !self.exp_policy.is_expired(s.entry_id, &s.storage) && !s.is_removed() {
                return Entry::Occupied(OccupiedEntry {
                    hash,
                    key: Some(key),
                    elem,
                    table: self,
                });
            }
        }
        Entry::Vacant(VacantEntry {
            hash,
            key,
            table: self,
        })
    }

    /// Clears the `HashMap`, returning all key-value pairs as an iterator. Keeps the allocated memory for reuse.
    ///
    /// When drop, this function also triggers internal [`ExpirePolicy::clear()`].
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "a", ());
    /// cache.insert(1, "b", ());
    ///
    /// for (k, v) in cache.drain() {
    ///     assert!(k == 0 || k == 1);
    ///     assert!(v == "a" || v == "b");
    /// }
    ///
    /// assert!(cache.is_empty());
    /// ```
    #[inline]
    pub fn drain(&mut self) -> Drain<'_, K, V, P> {
        self.exp_backlog = SegQueue::new();

        Drain {
            inner: self.table.drain(),
            policy: &mut self.exp_policy,
        }
    }
}

impl<K, V, P, H> HashMap<K, V, P, H>
where
    P: ExpirePolicy,
{
    /// Clears the `HashMap`, removing all key-value pairs. Keeps the allocated memory for reuse.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "a", ());
    /// cache.clear();
    /// assert!(cache.is_empty());
    /// ```
    #[inline]
    pub fn clear(&mut self) {
        self.table.clear();

        self.exp_backlog = SegQueue::new();
        self.exp_policy.clear();
        self.exp_bucket_table.clear();
    }

    /// Returns the exact number of elements in the `HashMap`.
    ///
    /// This functions is accurate than [`len_approx`] but uses slower algorithm `O(n)`.
    ///
    /// [`len_approx`]: #method.len_approx
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// assert_eq!(cache.len(), 0);
    /// cache.insert(0, "a", ());
    /// assert_eq!(cache.len(), 1);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Returns the number of elements in the `HashMap`.
    ///
    /// This function may contain count of elements in the `HashMap` that is not actually removed from internal table.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// assert_eq!(cache.len_approx(), 0);
    /// cache.insert(0, "a", ());
    /// assert_eq!(cache.len_approx(), 1);
    ///
    /// sleep(Duration::from_millis(10));
    /// assert_eq!(cache.len(), 0);
    /// assert_eq!(cache.len_approx(), 1);
    /// ```
    #[inline]
    pub fn len_approx(&self) -> usize {
        self.table.len()
    }

    ///
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::<u32, u32, _>::with_capacity(
    ///     20,
    ///     LazyFixedTTLPolicy::new(Duration::from_millis(10)),
    /// );
    /// assert!(cache.capacity() >= 20);
    /// ```
    #[inline]
    pub fn capacity(&self) -> usize {
        self.table.capacity()
    }

    /// Returns `true` if the `HashMap` contains no elements.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// assert!(cache.is_empty());
    /// cache.insert(0, "a", ());
    /// assert!(!cache.is_empty());
    ///
    /// sleep(Duration::from_millis(10));
    ///
    /// assert!(cache.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// An iterator visiting all key-value pairs in arbitrary order. The iterator element type is `(&'a K, &'a V)`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "a", ());
    /// cache.insert(1, "b", ());
    /// cache.insert(2, "c", ());
    ///
    /// for (k, v) in cache.iter() {
    ///     println!("key: {}, val: {}", k, v);
    /// }
    /// ```
    #[inline]
    pub fn iter(&self) -> Iter<'_, K, V, P> {
        unsafe {
            Iter {
                inner: self.table.iter(),
                policy: &self.exp_policy,
                marker: PhantomData,
            }
        }
    }

    /// An iterator visiting all key-value pairs in arbitrary order, with mutable references to the values. The iterator element type is `(&'a K, &'a mut V)`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, 0, ());
    /// cache.insert(1, 1, ());
    /// cache.insert(2, 2, ());
    ///
    /// for (_, v) in cache.iter_mut() {
    ///     *v *= 2;
    /// }
    ///
    /// for (k, v) in cache.iter() {
    ///     println!("key: {}, val: {}", k, v);
    /// }
    /// ```
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V, P> {
        unsafe {
            IterMut {
                inner: self.table.iter(),
                policy: &self.exp_policy,
                marker: PhantomData,
            }
        }
    }

    /// An iterator visiting all values in arbitrary order. The iterator element type is `&'a V`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "a", ());
    /// cache.insert(1, "b", ());
    /// cache.insert(2, "c", ());
    ///
    /// for v in cache.values() {
    ///     println!("{}", v);
    /// }
    /// ```
    #[inline]
    pub fn values(&self) -> Values<'_, K, V, P> {
        Values { inner: self.iter() }
    }

    /// An iterator visiting all values mutably in arbitrary order. The iterator element type is `&'a mut V`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, 0, ());
    /// cache.insert(1, 1, ());
    /// cache.insert(2, 2, ());
    ///
    /// for v in cache.values_mut() {
    ///     *v = *v + 6;
    /// }
    ///
    /// for v in cache.values() {
    ///     println!("{}", v);
    /// }
    /// ```
    #[inline]
    pub fn values_mut(&mut self) -> ValuesMut<'_, K, V, P> {
        ValuesMut {
            inner: self.iter_mut(),
        }
    }

    /// An iterator visiting all keys in arbitrary order. The iterator element type is `&'a K`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, "a", ());
    /// cache.insert(1, "b", ());
    /// cache.insert(2, "c", ());
    ///
    /// for v in cache.keys() {
    ///     println!("{}", v);
    /// }
    /// ```
    #[inline]
    pub fn keys(&self) -> Keys<'_, K, V, P> {
        Keys { inner: self.iter() }
    }
}

/// An An iterator over the entries of a `HashMap`.
///
/// This `struct` is created by the [`iter`] method on [`HashMap`]. See its documentation for more.
///
/// [`iter`]: HashMap::iter
#[derive(Clone)]
pub struct Iter<'a, K, V, P>
where
    P: ExpirePolicy,
{
    inner: RawIter<(K, V, Storage<P::Storage>)>,
    policy: &'a P,
    marker: PhantomData<(&'a K, &'a V, &'a P)>,
}

impl<'a, K, V, P> Iterator for Iter<'a, K, V, P>
where
    P: ExpirePolicy,
{
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((r, s)) = self
            .inner
            .next()
            .map(|v| unsafe { v.as_ref() })
            .map(|(k, v, s)| ((k, v), s))
        {
            if !self.policy.is_expired(s.entry_id, &s.storage) & !s.is_removed() {
                Some(r)
            } else {
                self.next()
            }
        } else {
            None
        }
    }
}

/// A mutable iterator over the entries of a `HashMap`.
///
/// This `struct` is created by the [`iter_mut`] method on [`HashMap`]. See its documentation for more.
///
/// [`iter_mut`]: HashMap::iter_mut
pub struct IterMut<'a, K, V, P>
where
    P: ExpirePolicy,
{
    inner: RawIter<(K, V, Storage<P::Storage>)>,
    policy: &'a P,
    marker: PhantomData<(&'a K, &'a V, &'a P)>,
}

impl<'a, K, V, P> Iterator for IterMut<'a, K, V, P>
where
    P: ExpirePolicy,
{
    type Item = (&'a mut K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((r, s)) = self
            .inner
            .next()
            .map(|v| unsafe { v.as_mut() })
            .map(|(k, v, s)| ((k, v), s))
        {
            if !self.policy.is_expired(s.entry_id, &s.storage) & !s.is_removed() {
                Some(r)
            } else {
                self.next()
            }
        } else {
            None
        }
    }
}

///An iterator over the keys of a `HashMap`.
///
/// This `struct` is created by the [`keys`] method on [`HashMap`]. See its documentation for more.
///
/// [`keys`]: HashMap::keys
pub struct Keys<'a, K, V, P>
where
    P: ExpirePolicy,
{
    inner: Iter<'a, K, V, P>,
}

impl<'a, K, V, P> Iterator for Keys<'a, K, V, P>
where
    P: ExpirePolicy,
{
    type Item = &'a K;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(k, _)| k)
    }
}

/// An iterator over the values of a `HashMap`.
///
/// This `struct` is created by the [`values`] method on [`HashMap`]. See its documentation for more.
///
/// [`values`]: HashMap::values
pub struct Values<'a, K, V, P>
where
    P: ExpirePolicy,
{
    inner: Iter<'a, K, V, P>,
}

impl<'a, K, V, P> Iterator for Values<'a, K, V, P>
where
    P: ExpirePolicy,
{
    type Item = &'a V;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(_, v)| v)
    }
}

/// A mutable iterator over the values of a `HashMap`.
///
/// This `struct` is created by the [`values_mut`] method on [`HashMap`]. See its documentation for more.
///
/// [`values_mut`]: HashMap::values_mut
pub struct ValuesMut<'a, K, V, P>
where
    P: ExpirePolicy,
{
    inner: IterMut<'a, K, V, P>,
}

impl<'a, K, V, P> Iterator for ValuesMut<'a, K, V, P>
where
    P: ExpirePolicy,
{
    type Item = &'a mut V;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(_, v)| v)
    }
}

/// A draining iterator over the entries of a `HashMap`.
///
/// This `struct` is created by the [`drain`] method on [`HashMap`]. See its documentation for more.
///
/// [`drain`]: HashMap::drain
pub struct Drain<'a, K, V, P>
where
    P: ExpirePolicy,
{
    inner: RawDrain<'a, (K, V, Storage<P::Storage>)>,
    policy: &'a mut P,
}

impl<'a, K, V, P> Iterator for Drain<'a, K, V, P>
where
    P: ExpirePolicy,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((r, s)) = self.inner.next().map(|(k, v, s)| ((k, v), s)) {
            if !self.policy.is_expired(s.entry_id, &s.storage) & !s.is_removed() {
                Some(r)
            } else {
                self.next()
            }
        } else {
            None
        }
    }
}

impl<'a, K, V, P> Drop for Drain<'a, K, V, P>
where
    P: ExpirePolicy,
{
    fn drop(&mut self) {
        self.policy.clear();
    }
}

/// A view into a single entry in a `HashMap`, which may either be vacant or occupied.
///
/// This enum is constructed from the [`entry`] method on [`HashMap`].
///
/// [`entry`]: HashMap::entry
pub enum Entry<'a, K, V, P, H>
where
    P: ExpirePolicy,
{
    Occupied(OccupiedEntry<'a, K, V, P, H>),
    Vacant(VacantEntry<'a, K, V, P, H>),
}

impl<'a, K, V, P, H> Entry<'a, K, V, P, H>
where
    K: Eq + Hash,
    P: ExpirePolicy,
    H: BuildHasher,
{
    /// Sets the value of the entry, and returns an [`OccupiedEntry`].
    ///
    /// # Example
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// let entry = cache.entry(0).insert("a", ());
    /// assert_eq!(entry.key(), &0);
    /// assert_eq!(entry.get(), &"a");
    ///
    /// assert_eq!(cache.get(&0), Some(&"a"));
    /// ```
    pub fn insert(self, value: V, init: P::Info) -> OccupiedEntry<'a, K, V, P, H> {
        match self {
            Entry::Occupied(mut entry) => {
                entry.insert(value, init);
                entry
            }
            Entry::Vacant(entry) => entry.insert_entry(value, init),
        }
    }

    /// Ensures a value is in the entry by inserting the default if empty,
    /// and returns a mutable reference to the value in the entry.
    ///
    /// # Example
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache.entry(0).or_insert(0, ());
    /// assert_eq!(cache.get(&0), Some(&0));
    ///
    /// *cache.entry(0).or_insert(200, ()) += 10;
    /// assert_eq!(cache.get(&0), Some(&10));
    ///
    /// sleep(Duration::from_millis(10));
    ///
    /// cache.entry(0).or_insert(100, ());
    /// assert_eq!(cache.get(&0), Some(&100));
    /// ```
    pub fn or_insert(self, default: V, init: P::Info) -> &'a mut V {
        match self {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(default, init),
        }
    }

    /// Ensures a value is in the entry by inserting the result of the default function if empty,
    /// and returns a mutable reference to the value in the entry.
    ///
    /// # Example
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache.entry("a").or_insert_with(|| "A".to_string(), ());
    /// assert_eq!(cache.get(&"a"), Some(&"A".to_string()));
    ///
    /// cache
    ///     .entry("a")
    ///     .or_insert_with(|| "B".to_string(), ())
    ///     .push('B');
    /// assert_eq!(cache.get(&"a"), Some(&"AB".to_string()));
    ///
    /// sleep(Duration::from_millis(10));
    ///
    /// cache.entry("a").or_insert_with(|| "C".to_string(), ());
    /// assert_eq!(cache.get(&"a"), Some(&"C".to_string()));
    /// ```
    pub fn or_insert_with<F: FnOnce() -> V>(self, default: F, init: P::Info) -> &'a mut V {
        match self {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(default(), init),
        }
    }

    /// Ensures a value is in the entry by inserting, if empty, the result of the default function.
    /// This method allows for generating key-derived values for insertion by providing the default function a reference to the key that was moved during the .entry(key) method call.
    ///
    /// The reference to the moved key is provided so that cloning or copying the key is unnecessary, unlike with .or_insert_with(|| ... ).
    ///
    /// # Example
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache
    ///     .entry("endorphin")
    ///     .or_insert_with_key(|k| k.chars().count(), ());
    ///
    /// assert_eq!(cache.get(&"endorphin"), Some(&9));
    /// ```
    pub fn or_insert_with_key<F: FnOnce(&K) -> V>(self, default: F, init: P::Info) -> &'a mut V {
        match self {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let value = default(entry.key());
                entry.insert(value, init)
            }
        }
    }

    /// Returns a reference to this entry’s key.
    ///
    /// # Example
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache =
    ///     HashMap::<&str, u32, _>::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// assert_eq!(cache.entry("library").key(), &"library");
    /// ```
    pub fn key(&self) -> &K {
        match *self {
            Entry::Occupied(ref entry) => entry.key(),
            Entry::Vacant(ref entry) => entry.key(),
        }
    }

    /// Provides in-place mutable access to an occupied entry before any potential inserts into the `HashMap`.
    ///
    /// # Example
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache
    ///     .entry("endorphin")
    ///     .and_modify(|v| unreachable!())
    ///     .or_insert(0, ());
    /// assert_eq!(cache.get(&"endorphin"), Some(&0));
    ///
    /// cache
    ///     .entry("endorphin")
    ///     .and_modify(|v| *v += 10)
    ///     .or_insert(123, ());
    /// assert_eq!(cache.get(&"endorphin"), Some(&10));
    /// ```
    pub fn and_modify<F>(self, f: F) -> Self
    where
        F: FnOnce(&mut V),
    {
        match self {
            Entry::Occupied(mut entry) => {
                f(entry.get_mut());
                Entry::Occupied(entry)
            }
            Entry::Vacant(entry) => Entry::Vacant(entry),
        }
    }

    /// Provides shared access to the key and owned access to the value of an occupied entry
    /// and allows to replace or remove it based on the value of the returned option.
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// match cache
    ///     .entry(10)
    ///     .and_replace_entry_with(|_k, _v| unreachable!())
    /// {
    ///     Entry::Occupied(_) => unreachable!(),
    ///     Entry::Vacant(entry) => assert_eq!(entry.key(), &10),
    /// }
    ///
    /// cache.insert(10, 200, ());
    ///
    /// match cache.entry(10).and_replace_entry_with(|k, v| Some(k + v)) {
    ///     Entry::Occupied(entry) => assert_eq!(entry.get(), &210),
    ///     Entry::Vacant(_) => unreachable!(),
    /// }
    /// ```
    pub fn and_replace_entry_with<F>(self, f: F) -> Self
    where
        F: FnOnce(&K, V) -> Option<V>,
    {
        match self {
            Entry::Occupied(entry) => entry.replace_entry_with(f),
            Entry::Vacant(_) => self,
        }
    }
}

impl<'a, K, V, P, H> Entry<'a, K, V, P, H>
where
    K: Eq + Hash,
    V: Default,
    P: ExpirePolicy,
    H: BuildHasher,
{
    /// Ensures a value is in the entry by inserting the default value if empty, and returns a mutable reference to the value in the entry.
    ///
    /// # Example
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::<_, u32, _>::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache.entry("endorphin").or_default(());
    ///
    /// assert_eq!(cache.get(&"endorphin"), Some(&u32::default()));
    /// ```
    pub fn or_default(self, init: P::Info) -> &'a mut V
    where
        V: Default,
    {
        match self {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(Default::default(), init),
        }
    }
}

/// A view into an occupied entry in a `HashMap`. It is part of the [`Entry`] enum.
pub struct OccupiedEntry<'a, K, V, P, H>
where
    P: ExpirePolicy,
{
    hash: u64,
    key: Option<K>,
    elem: Bucket<(K, V, Storage<P::Storage>)>,
    table: &'a mut HashMap<K, V, P, H>,
}

impl<'a, K, V, P, H> OccupiedEntry<'a, K, V, P, H>
where
    P: ExpirePolicy,
    K: Eq + Hash,
    H: BuildHasher,
{
    /// Gets a reference to the key in the entry.
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache.entry("occupied").or_insert("entry", ());
    ///
    /// if let Entry::Occupied(entry) = cache.entry("occupied") {
    ///     assert_eq!(entry.key(), &"occupied");
    /// }
    /// ```
    pub fn key(&self) -> &K {
        unsafe { &self.elem.as_ref().0 }
    }

    /// Take the ownership of the key and value from the `HashMap`.
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.entry("cache").or_insert("library", ());
    ///
    /// if let Entry::Occupied(entry) = cache.entry("cache") {
    ///     assert_eq!(entry.remove_entry(), ("cache", "library"));
    /// };
    ///
    /// assert_eq!(cache.contains_key("cache"), false);
    /// ```
    pub fn remove_entry(self) -> (K, V) {
        let key = unsafe { &self.elem.as_ref().0 };
        self.table.remove_entry(key).unwrap()
    }

    /// Gets a reference to the value in the entry.
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache.insert("cache", "library", ());
    ///
    /// if let Entry::Occupied(entry) = cache.entry("cache") {
    ///     assert_eq!(entry.get(), &"library");
    /// }
    /// ```
    pub fn get(&self) -> &V {
        let (_, v, s) = unsafe { self.elem.as_mut() };
        self.table
            .handle_status(self.table.exp_policy.on_access(s.entry_id, &mut s.storage));
        v
    }

    /// Gets a mutable reference to the value in the entry.
    ///
    /// If you need a reference to the `OccupiedEntry` which may outlive the destruction of the Entry value, see [`into_mut`].
    ///
    /// [`into_mut`]: #method.into_mut
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache.insert("a", 100, ());
    ///
    /// if let Entry::Occupied(mut entry) = cache.entry("a") {
    ///     *entry.get_mut() *= 2;
    ///     assert_eq!(*entry.get(), 200);
    ///
    ///     *entry.get_mut() += 22;
    /// }
    ///
    /// assert_eq!(cache.get(&"a"), Some(&222));
    /// ```
    pub fn get_mut(&mut self) -> &mut V {
        let (_, v, s) = unsafe { self.elem.as_mut() };
        self.table
            .handle_status(self.table.exp_policy.on_access(s.entry_id, &mut s.storage));
        v
    }

    /// Converts the `OccupiedEntry` into a mutable reference to the value in the entry with a lifetime bound to the `HashMap` itself.
    ///
    /// If you need multiple references to the `OccupiedEntry`, see [`get_mut`].
    ///
    /// [`get_mut`]: #method.get_mut
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache.insert("a", 100, ());
    ///
    /// let v = match cache.entry("a") {
    ///     Entry::Occupied(entry) => entry.into_mut(),
    ///     Entry::Vacant(_) => unreachable!(),
    /// };
    ///
    /// *v += 11;
    ///
    /// assert_eq!(cache.get(&"a"), Some(&111));
    /// ```
    pub fn into_mut(self) -> &'a mut V {
        let (_, v, s) = unsafe { self.elem.as_mut() };
        self.table
            .handle_status(self.table.exp_policy.on_access(s.entry_id, &mut s.storage));
        v
    }

    /// Sets the value of the entry, and returns the entry’s old value.
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache.insert("a", 100, ());
    ///
    /// if let Entry::Occupied(mut entry) = cache.entry("a") {
    ///     assert_eq!(entry.insert(200, ()), 100);
    /// }
    ///
    /// assert_eq!(cache.get(&"a"), Some(&200));
    /// ```
    pub fn insert(&mut self, value: V, init: P::Info) -> V {
        let k = unsafe { &self.elem.as_ref().0 };

        let s = self.table.exp_policy.init_storage(init);
        let mut storage = Storage::new(s, self.table.exp_bucket_table.acquire_slot());

        self.table.handle_status(
            self.table
                .exp_policy
                .on_insert(storage.entry_id, &mut storage.storage),
        );

        let (_, old_v, old_s) = self
            .table
            .table
            .get_mut(self.hash, equivalent_key(k))
            .unwrap();
        let bucket = self.table.exp_bucket_table.get(old_s.entry_id).unwrap();

        self.table.exp_bucket_table.set_bucket(old_s.entry_id, None);
        self.table
            .exp_bucket_table
            .set_bucket(storage.entry_id, Some(bucket.clone()));

        let _ = mem::replace(old_s, storage);
        let old_v = mem::replace(old_v, value);

        old_v
    }

    /// Takes the value out of the entry, and returns it.
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache.insert("a", 100, ());
    ///
    /// if let Entry::Occupied(entry) = cache.entry("a") {
    ///     assert_eq!(entry.remove(), 100);
    /// }
    ///
    /// assert_eq!(cache.contains_key("a"), false);
    /// ```
    pub fn remove(self) -> V {
        self.table.remove(unsafe { &self.elem.as_ref().0 }).unwrap()
    }

    /// Replaces the entry, returning the old key and value. The new key in the hash map will be the key used to create this entry.
    ///
    /// # Panics
    /// Will panic if this `OccupiedEntry` was created through [`Entry::insert`].
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// let key: Vec<u32> = Vec::new();
    ///
    /// cache.insert(key.clone(), 500, ());
    ///
    /// if let Entry::Occupied(entry) = cache.entry(Vec::with_capacity(10)) {
    ///     let (old_k, old_v) = entry.replace_entry(200);
    ///     assert_eq!(old_k.capacity(), 0);
    ///     assert_eq!(old_v, 500);
    /// }
    ///
    /// if let Entry::Occupied(entry) = cache.entry(key.clone()) {
    ///     let (k, v) = entry.remove_entry();
    ///     assert!(k.capacity() >= 10);
    ///     assert_eq!(v, 200);
    /// }
    /// ```
    pub fn replace_entry(self, value: V) -> (K, V) {
        let entry = unsafe { self.elem.as_mut() };

        self.table.handle_status(
            self.table
                .exp_policy
                .on_access(entry.2.entry_id, &mut entry.2.storage),
        );

        let old_key = mem::replace(&mut entry.0, self.key.unwrap());
        let old_value = mem::replace(&mut entry.1, value);

        (old_key, old_value)
    }

    /// Replaces the key in the `HashMap` with the key used to create this entry.
    ///
    /// # Panics
    /// Will panic if this `OccupiedEntry` was created through [`Entry::insert`].
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// let key: Vec<u32> = Vec::new();
    ///
    /// cache.insert(key.clone(), (), ());
    ///
    /// if let Entry::Occupied(entry) = cache.entry(Vec::with_capacity(10)) {
    ///     let old_k = entry.replace_key();
    ///     assert_eq!(old_k.capacity(), 0);
    /// }
    ///
    /// if let Entry::Occupied(entry) = cache.entry(key.clone()) {
    ///     let (k, _) = entry.remove_entry();
    ///     assert!(k.capacity() >= 10);
    /// }
    /// ```
    pub fn replace_key(self) -> K {
        let entry = unsafe { self.elem.as_mut() };
        self.table.handle_status(
            self.table
                .exp_policy
                .on_access(entry.2.entry_id, &mut entry.2.storage),
        );

        mem::replace(&mut entry.0, self.key.unwrap())
    }

    /// Provides shared access to the key and owned access to the value of the entry
    /// and allows to replace or remove it based on the value of the returned option.
    ///
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// cache.insert(11, 22, ());
    ///
    /// let result = match  cache.entry(11) {
    ///     Entry::Occupied(entry) => entry.replace_entry_with(|k, v| {
    ///         assert_eq!(k, &11);
    ///         assert_eq!(v, 22);
    ///         Some(k + v)
    ///     }),
    ///     Entry::Vacant(_) => unreachable!(),
    /// };
    ///
    /// match result {
    ///     Entry::Occupied(e) => {
    ///         assert_eq!(e.get(), &33);
    ///     }
    ///     Entry::Vacant(_) => unreachable!(),
    /// };
    ///
    /// assert_eq!(cache.get(&11), Some(&33));
    ///
    /// let result = match cache.entry(11) {
    ///     Entry::Occupied(entry) => entry.replace_entry_with(|_k, _v| None),
    ///     Entry::Vacant(_) => unreachable!(),
    /// };
    ///
    /// match result {
    ///     Entry::Occupied(_) => unreachable!(),
    ///     Entry::Vacant(_) => {
    ///         assert_eq!(cache.get(&11), None);
    ///     }
    /// };
    /// ```
    pub fn replace_entry_with<F>(self, f: F) -> Entry<'a, K, V, P, H>
    where
        F: FnOnce(&K, V) -> Option<V>,
    {
        unsafe {
            let mut spare_key = None;

            let elem = self.elem.clone();
            let s = &mut elem.clone().as_mut().2;

            self.table.exp_bucket_table.set_bucket(s.entry_id, None);

            self.table
                .table
                .replace_bucket_with(elem.clone(), |(key, value, policy)| {
                    if let Some(new_value) = f(&key, value) {
                        Some((key, new_value, policy))
                    } else {
                        spare_key = Some(key);
                        None
                    }
                });

            self.table
                .exp_bucket_table
                .set_bucket(s.entry_id, Some(elem.clone()));
            self.table
                .handle_status(self.table.exp_policy.on_access(s.entry_id, &mut s.storage));

            if let Some(key) = spare_key {
                Entry::Vacant(VacantEntry {
                    hash: self.hash,
                    key,
                    table: self.table,
                })
            } else {
                Entry::Occupied(self)
            }
        }
    }
}

pub struct VacantEntry<'a, K, V, P, H>
where
    P: ExpirePolicy,
{
    hash: u64,
    key: K,
    table: &'a mut HashMap<K, V, P, H>,
}

impl<'a, K, V, P, H> VacantEntry<'a, K, V, P, H>
where
    P: ExpirePolicy,
    K: Eq,
{
    /// Gets a reference to the key that would be used when inserting a value through the `VacantEntry`.
    /// 
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::<_, u32, _>::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// 
    /// match cache.entry("vacant") {
    ///     Entry::Occupied(_) => unreachable!(),
    ///     Entry::Vacant(entry) => assert_eq!(entry.key(), &"vacant"),
    /// }
    /// ```
    pub fn key(&self) -> &K {
        &self.key
    }

    /// Take ownership of the key.
    /// 
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::<_, u32, _>::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// 
    /// match cache.entry("vacant") {
    ///     Entry::Occupied(_) => unreachable!(),
    ///     Entry::Vacant(entry) => assert_eq!(entry.into_key(), "vacant"),
    /// }
    /// ```
    pub fn into_key(self) -> K {
        self.key
    }

    /// Sets the value of the entry with the `VacantEntry`’s key, and returns a mutable reference to it.
    /// 
    /// # Example
    /// ```
    /// use endorphin::map::Entry;
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashMap;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// 
    /// let v = match cache.entry("hello") {
    ///     Entry::Occupied(_) => unreachable!(),
    ///     Entry::Vacant(entry) => entry.insert("rust".to_string(), ()),
    /// };
    /// 
    /// v.push_str("acean");
    /// 
    /// assert_eq!(cache.get(&"hello"), Some(&"rustacean".to_string()));
    /// ```
    pub fn insert(self, value: V, init: P::Info) -> &'a mut V
    where
        K: Hash,
        H: BuildHasher,
    {
        let bucket = unsafe { self.table.raw_insert(self.key, value, init).0 };

        unsafe { &mut bucket.as_mut().1 }
    }

    fn insert_entry(self, value: V, init: P::Info) -> OccupiedEntry<'a, K, V, P, H>
    where
        K: Hash,
        H: BuildHasher,
    {
        let elem = unsafe { self.table.raw_insert(self.key, value, init).0 };

        OccupiedEntry {
            hash: self.hash,
            key: None,
            elem,
            table: self.table,
        }
    }
}

#[cfg(test)]
mod test_map {
    use super::HashMap;
    use crate::policy::{Command, ExpirePolicy};
    use crate::EntryId;

    struct MockPolicy {}
    impl MockPolicy {
        fn new() -> Self {
            Self {}
        }
    }

    impl ExpirePolicy for MockPolicy {
        type Info = ();

        type Storage = ();

        fn init_storage(&self, _: Self::Info) -> Self::Storage {
            ()
        }

        fn clear(&mut self) {}

        fn is_expired(&self, _: EntryId, _: &Self::Storage) -> bool {
            false
        }

        fn on_access(&self, _: EntryId, _: &Self::Storage) -> Command {
            Command::Noop
        }

        fn on_insert(&self, _: EntryId, _: &Self::Storage) -> Command {
            Command::Noop
        }

        fn on_resize(&self) -> Command {
            Command::Noop
        }
    }

    #[test]
    fn test_zero_capacities() {
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.capacity(), 0);

        map.insert(1, 1, ());
        map.insert(2, 2, ());
        map.remove(&1);
        map.remove(&2);
        map.shrink_to_fit();

        assert_eq!(map.capacity(), 0);
    }

    #[test]
    fn test_insert() {
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.len(), 0);
        assert!(map.insert(0, 0, ()).is_none());
        assert_eq!(map.len(), 1);
        assert!(map.insert(1, 1, ()).is_none());
        assert_eq!(map.len(), 2);

        assert!(map.insert(1, 2, ()).is_some());
        assert_eq!(map.len(), 2);

        assert_eq!(map.get(&0).unwrap(), &0);
        assert_eq!(map.get(&1).unwrap(), &2);
    }

    #[test]
    fn test_get() {
        let mut map = HashMap::new(MockPolicy::new());

        assert!(map.get(&0).is_none());
        assert!(map.get(&1).is_none());

        assert!(map.insert(0, 0, ()).is_none());
        assert!(map.insert(1, 1, ()).is_none());

        assert_eq!(map.get(&0).unwrap(), &0);
        assert_eq!(map.get(&1).unwrap(), &1);
    }

    #[test]
    fn test_get_mut() {
        let mut map = HashMap::new(MockPolicy::new());

        assert!(map.get_mut(&0).is_none());
        assert!(map.get_mut(&1).is_none());

        assert!(map.insert(0, 0, ()).is_none());
        assert!(map.insert(1, 1, ()).is_none());

        let v0 = map.get_mut(&0).unwrap();
        let v1 = map.get_mut(&1).unwrap();

        assert_eq!(v0, &0);
        assert_eq!(v1, &1);

        *v0 = 10;
        *v1 = 20;

        assert_eq!(map.get(&0).unwrap(), &10);
        assert_eq!(map.get(&1).unwrap(), &20);
    }

    #[test]
    fn test_get_key_value() {
        let mut map = HashMap::new(MockPolicy::new());

        assert!(map.get_key_value(&0).is_none());
        assert!(map.get_key_value(&1).is_none());

        assert!(map.insert(0, 0, ()).is_none());
        assert!(map.insert(1, 1, ()).is_none());

        assert_eq!(map.get_key_value(&0).unwrap(), (&0, &0));
        assert_eq!(map.get_key_value(&1).unwrap(), (&1, &1));
    }

    #[test]
    fn test_contains_key() {
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.contains_key(&0), false);
        assert_eq!(map.contains_key(&1), false);

        assert!(map.insert(0, 0, ()).is_none());

        assert_eq!(map.contains_key(&0), true);
        assert_eq!(map.contains_key(&1), false);

        assert!(map.remove(&0).is_some());

        assert_eq!(map.contains_key(&0), false);
    }

    #[test]
    fn test_remove() {
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.contains_key(&0), false);

        assert!(map.insert(0, 1, ()).is_none());

        assert_eq!(map.contains_key(&0), true);

        assert_eq!(map.remove(&0).unwrap(), 1);

        assert_eq!(map.contains_key(&0), false);
    }

    #[test]
    fn test_remove_entry() {
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.contains_key(&0), false);

        assert!(map.insert(0, 1, ()).is_none());

        assert_eq!(map.contains_key(&0), true);

        assert_eq!(map.remove_entry(&0).unwrap(), (0, 1));

        assert_eq!(map.contains_key(&0), false);
    }

    #[test]
    fn test_reserve() {
        let mut map = HashMap::<u32, u32, _>::new(MockPolicy::new());

        assert_eq!(map.capacity(), 0);

        map.reserve(10);

        assert!(map.capacity() > 0);
    }

    #[test]
    fn test_shrink_to_fit() {
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.capacity(), 0);

        map.insert(0, 0, ());
        map.insert(1, 1, ());

        map.shrink_to_fit();

        assert!(map.capacity() >= 2);

        map.remove(&0);
        map.remove(&1);
        map.shrink_to_fit();

        assert_eq!(map.capacity(), 0);
    }

    #[test]
    fn test_shrink_to() {
        let mut map = HashMap::new(MockPolicy::new());

        map.reserve(32);

        assert!(map.capacity() >= 32);

        map.shrink_to(0);

        assert_eq!(map.capacity(), 0);

        map.insert(0, 0, ());
        map.insert(1, 1, ());
        map.insert(2, 2, ());
        map.insert(3, 3, ());

        map.shrink_to(0);
        assert!(map.capacity() >= 4);
    }

    #[test]
    fn test_drain() {
        let mut map = HashMap::new(MockPolicy::new());

        map.insert(1, 1, ());
        map.insert(2, 2, ());
        map.insert(3, 3, ());
        map.insert(4, 4, ());

        {
            let mut sum = 0;

            let drains = map.drain();
            for e in drains {
                sum += e.0;
            }

            assert_eq!(sum, 10);
        }

        assert!(map.is_empty());
    }

    #[test]
    fn test_clear() {
        let mut map = HashMap::new(MockPolicy::new());

        assert!(map.is_empty());
        assert_eq!(map.capacity(), 0);

        map.insert(1, 1, ());
        map.insert(2, 2, ());
        map.insert(3, 3, ());
        map.insert(4, 4, ());

        assert!(!map.is_empty());

        map.clear();

        assert!(map.is_empty());
        assert!(map.capacity() > 0);
    }

    #[test]
    fn test_len() {
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.len(), 0);

        map.insert(1, 1, ());
        assert_eq!(map.len(), 1);

        map.insert(2, 2, ());
        assert_eq!(map.len(), 2);

        map.remove(&1);
        assert_eq!(map.len(), 1);

        map.remove(&2);
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn test_entry() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());
        map.insert(0, 0, ());

        match map.entry(0) {
            Entry::Occupied(_) => assert!(true),
            Entry::Vacant(_) => unreachable!(),
        }

        match map.entry(1) {
            Entry::Occupied(_) => unreachable!(),
            Entry::Vacant(_) => assert!(true),
        }
    }

    #[test]
    fn test_entry_insert() {
        let mut map = HashMap::new(MockPolicy::new());

        let mut entry = map.entry(0).insert("0", ());
        assert_eq!(entry.get(), &"0");

        entry.insert("1", ());
        assert_eq!(map.get(&0).unwrap(), &"1");
    }

    #[test]
    fn test_entry_or_insert() {
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.entry(0).or_insert(0, ()), &0);
        assert_eq!(map.get(&0).unwrap(), &0);

        *map.entry(0).or_insert(0, ()) += 5;
        assert_eq!(map.entry(0).or_insert(0, ()), &5);
        assert_eq!(map.get(&0).unwrap(), &5);
    }

    #[test]
    fn test_entry_or_insert_with() {
        let five = || 5;

        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.entry(0).or_insert_with(five, ()), &five());

        *map.entry(0).or_insert_with(five, ()) *= 2;

        assert_eq!(map.entry(0).or_insert_with(five, ()), &(five() * 2));
        assert_eq!(map.get(&0).unwrap(), &(five() * 2));
    }

    #[test]
    fn test_entry_or_insert_with_key() {
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.entry(0).or_insert_with_key(|x| x + 1, ()), &1);
        *map.entry(0).or_insert_with_key(|x| x + 1, ()) += 5;

        assert_eq!(map.entry(0).or_insert_with_key(|x| x + 1, ()), &6);
        assert_eq!(map.get(&0).unwrap(), &6);
    }

    #[test]
    fn test_entry_key() {
        let mut map = HashMap::<_, u32, _>::new(MockPolicy::new());

        for i in 0..100 {
            assert_eq!(map.entry(i).key(), &i);
        }
    }

    #[test]
    fn test_entry_and_modify() {
        let mut map = HashMap::new(MockPolicy::new());
        map.insert(0, 0, ());

        let now = map
            .entry(0)
            .and_modify(|v| *v = *v + 5)
            .and_modify(|v| *v = *v * 10);
        assert_eq!(now.or_insert(0, ()), &50);
        assert_eq!(map.get(&0).unwrap(), &50);
    }

    #[test]
    fn test_entry_and_replace_entry_with() {
        let mut map = HashMap::new(MockPolicy::new());
        map.insert(1, 10, ());

        map.entry(1).and_replace_entry_with(|k, v| Some(k + v));

        assert_eq!(map.get(&1).unwrap(), &11);
    }

    #[test]
    fn test_occupied_entry_key() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());
        map.insert(1, 10, ());

        match map.entry(1) {
            Entry::Occupied(entry) => assert_eq!(entry.key(), &1),
            Entry::Vacant(_) => unreachable!(),
        }
    }

    #[test]
    fn test_occupied_entry_remove_entry() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());
        map.insert(1, 10, ());

        match map.entry(1) {
            Entry::Occupied(entry) => assert_eq!(entry.remove_entry(), (1, 10)),
            Entry::Vacant(_) => unreachable!(),
        }

        assert_eq!(map.get(&1), None);
    }

    #[test]
    fn test_occupied_entry_get() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());
        map.insert(1, 10, ());

        match map.entry(1) {
            Entry::Occupied(entry) => assert_eq!(entry.get(), &10),
            Entry::Vacant(_) => unreachable!(),
        }
    }

    #[test]
    fn test_occupied_entry_get_mut() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());
        map.insert(1, 10, ());

        match map.entry(1) {
            Entry::Occupied(mut entry) => {
                let v = entry.get_mut();
                assert_eq!(v, &10);
                *v += 10;

                let v = entry.get_mut(); // not moved
            }
            Entry::Vacant(_) => unreachable!(),
        }

        assert_eq!(map.get(&1).unwrap(), &20);
    }

    #[test]
    fn test_occupied_entry_into_mut() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());
        map.insert(1, 10, ());

        match map.entry(1) {
            Entry::Occupied(entry) => {
                let v = entry.into_mut();
                assert_eq!(v, &10);
                *v += 10;
            }
            Entry::Vacant(_) => unreachable!(),
        }

        assert_eq!(map.get(&1).unwrap(), &20);
    }

    #[test]
    fn test_occupied_entry_insert() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());
        map.insert(1, 10, ());

        assert_eq!(map.get(&1).unwrap(), &10);

        match map.entry(1) {
            Entry::Occupied(mut entry) => assert_eq!(entry.insert(20, ()), 10),
            Entry::Vacant(_) => unreachable!(),
        }

        assert_eq!(map.get(&1).unwrap(), &20);
    }

    #[test]
    fn test_occupied_entry_remove() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());
        map.insert(1, 10, ());

        match map.entry(1) {
            Entry::Occupied(entry) => assert_eq!(entry.remove(), 10),
            Entry::Vacant(_) => unreachable!(),
        }

        assert_eq!(map.get(&1), None);
    }

    #[test]
    fn test_occupied_entry_replace_entry() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());

        map.insert(0, Vec::<u32>::new(), ());

        match map.entry(0) {
            Entry::Occupied(entry) => {
                let old = entry.replace_entry(Vec::with_capacity(10));
                assert_eq!(old.0, 0);
                assert_eq!(old.1.capacity(), 0);
            }
            Entry::Vacant(_) => unreachable!(),
        }

        assert_eq!(map.get(&0).unwrap().capacity(), 10);
    }

    #[test]
    fn test_occupied_entry_replace_key() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());

        let vec = Vec::<u32>::new();

        map.insert(vec.clone(), (), ());

        match map.entry(Vec::with_capacity(10)) {
            Entry::Occupied(entry) => {
                let old = entry.replace_key();
                assert_eq!(old.capacity(), 0);
            }
            Entry::Vacant(_) => unreachable!(),
        }

        match map.entry(Vec::with_capacity(3)) {
            Entry::Occupied(entry) => assert_eq!(entry.key().capacity(), 10),
            Entry::Vacant(_) => unreachable!(),
        }
    }

    #[test]
    fn test_occupied_entry_replace_entry_with() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());

        map.insert(Vec::<u32>::new(), 10, ());

        let entry = match map.entry(Vec::new()) {
            Entry::Occupied(entry) => entry.replace_entry_with(|k, v| {
                assert_eq!(k.capacity(), 0);
                assert_eq!(v, 10);
                Some(v * 10)
            }),
            Entry::Vacant(_) => unreachable!(),
        };

        match entry {
            Entry::Occupied(entry) => {
                assert_eq!(entry.key().capacity(), 0);
                assert_eq!(entry.get(), &100)
            }
            Entry::Vacant(_) => unreachable!(),
        }

        let entry = match map.entry(Vec::new()) {
            Entry::Occupied(entry) => entry.replace_entry_with(|k, v| {
                assert_eq!(k.capacity(), 0);
                assert_eq!(v, 100);
                None
            }),
            Entry::Vacant(_) => unreachable!(),
        };

        match entry {
            Entry::Occupied(_) => unreachable!(),
            Entry::Vacant(_) => {
                assert_eq!(map.get(&Vec::new()), None)
            }
        };
    }

    #[test]
    #[should_panic]
    fn test_occupied_entry_insert_result_replace_key() {
        let mut map = HashMap::new(MockPolicy::new());

        map.entry(0).insert(0, ()).replace_key();
    }

    #[test]
    #[should_panic]
    fn test_occupied_entry_insert_result_replace_entry() {
        let mut map = HashMap::new(MockPolicy::new());

        map.entry(0).insert(0, ()).replace_entry(1);
    }

    #[test]
    fn test_vacant_entry_key() {
        use super::Entry;
        let mut map = HashMap::<u32, u32, _>::new(MockPolicy::new());

        match map.entry(0) {
            Entry::Occupied(_) => unreachable!(),
            Entry::Vacant(entry) => {
                assert_eq!(entry.key(), &0)
            }
        }
    }

    #[test]
    fn test_vacant_entry_insert() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.get(&0), None);

        match map.entry(0) {
            Entry::Occupied(_) => unreachable!(),
            Entry::Vacant(entry) => {
                let v = entry.insert(10, ());
                *v += 20;
            }
        }

        assert_eq!(map.get(&0).unwrap(), &30);
    }

    #[test]
    fn test_vacant_entry_insert_entry() {
        use super::Entry;
        let mut map = HashMap::new(MockPolicy::new());

        assert_eq!(map.get(&0), None);

        let entry = match map.entry(0) {
            Entry::Occupied(_) => unreachable!(),
            Entry::Vacant(entry) => entry.insert_entry(33, ()),
        };

        assert_eq!(entry.get(), &33);
        assert_eq!(map.get(&0).unwrap(), &33);
    }
}
