use crate::{map, policy::ExpirePolicy, HashMap};

use hashbrown::hash_map::DefaultHashBuilder;

use std::{
    borrow::Borrow,
    fmt,
    hash::{BuildHasher, Hash},
    iter::{Chain, FromIterator},
};

/// A hash set with an expiration policy, built on top of [`HashMap`] with `()` values.
///
/// Every element carries its own expiration state, so an element can vanish between two calls
/// without anyone removing it. That makes the set operations below a snapshot of the elements
/// that are alive while the iterator runs, not a stable mathematical set.
///
/// # Example
///
/// ```
/// use endorphin::policy::TTLPolicy;
/// use endorphin::HashSet;
///
/// use std::thread::sleep;
/// use std::time::Duration;
///
/// let mut cache = HashSet::new(TTLPolicy::new());
///
/// cache.insert("Still", Duration::from_secs(3));
/// cache.insert("Gonna", Duration::from_secs(1));
///
/// sleep(Duration::from_secs(1));
///
/// assert!(cache.contains(&"Still"));
/// assert!(!cache.contains(&"Gonna"));
/// ```
pub struct HashSet<T, P, H = DefaultHashBuilder>
where
    P: ExpirePolicy,
{
    map: HashMap<T, (), P, H>,
}

impl<T, P> HashSet<T, P, DefaultHashBuilder>
where
    P: ExpirePolicy,
{
    /// Creates an empty `HashSet` driven by `policy`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache: HashSet<u32, _, _> =
    ///     HashSet::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    ///
    /// assert!(cache.is_empty());
    /// ```
    pub fn new(policy: P) -> Self {
        Self {
            map: HashMap::new(policy),
        }
    }

    /// Creates an empty `HashSet` with at least the given capacity, driven by `policy`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let cache: HashSet<u32, _, _> =
    ///     HashSet::with_capacity(20, LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    ///
    /// assert!(cache.capacity() >= 20);
    /// ```
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
    /// Reserves capacity for at least `additional` more elements.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache: HashSet<u32, _, _> =
    ///     HashSet::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    /// cache.reserve(10);
    ///
    /// assert!(cache.capacity() >= 10);
    /// ```
    pub fn reserve(&mut self, additional: usize) {
        self.map.reserve(additional)
    }

    #[inline]
    /// Shrinks the capacity of the set as much as possible.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache: HashSet<u32, _, _> =
    ///     HashSet::with_capacity(100, LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    ///
    /// cache.insert(0, ());
    /// cache.shrink_to_fit();
    ///
    /// assert!(cache.capacity() >= 1);
    /// assert!(cache.contains(&0));
    /// ```
    pub fn shrink_to_fit(&mut self) {
        self.map.shrink_to_fit()
    }

    #[inline]
    /// Shrinks the capacity of the set with a lower bound.
    ///
    /// The capacity stays at least as large as both the length and `min_capacity`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache: HashSet<u32, _, _> =
    ///     HashSet::with_capacity(100, LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    ///
    /// cache.insert(0, ());
    /// cache.shrink_to(10);
    ///
    /// assert!(cache.capacity() >= 10);
    /// ```
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.map.shrink_to(min_capacity)
    }

    #[inline]
    /// Visits the elements that are in `self` but not in `other`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let policy = || LazyFixedTTLPolicy::new(Duration::from_secs(30));
    ///
    /// let mut a = HashSet::new(policy());
    /// let mut b = HashSet::new(policy());
    ///
    /// a.insert(1, ());
    /// a.insert(2, ());
    /// b.insert(2, ());
    /// b.insert(3, ());
    ///
    /// let mut diff: Vec<_> = a.difference(&b).copied().collect();
    /// diff.sort_unstable();
    ///
    /// assert_eq!(diff, [1]);
    /// ```
    pub fn difference<'a>(&'a self, other: &'a Self) -> Difference<'a, T, P, H> {
        Difference {
            iter: self.iter(),
            other,
        }
    }

    #[inline]
    /// Visits the elements that are in `self` or in `other`, but not in both.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let policy = || LazyFixedTTLPolicy::new(Duration::from_secs(30));
    ///
    /// let mut a = HashSet::new(policy());
    /// let mut b = HashSet::new(policy());
    ///
    /// a.insert(1, ());
    /// a.insert(2, ());
    /// b.insert(2, ());
    /// b.insert(3, ());
    ///
    /// let mut sym: Vec<_> = a.symmetric_difference(&b).copied().collect();
    /// sym.sort_unstable();
    ///
    /// assert_eq!(sym, [1, 3]);
    /// ```
    pub fn symmetric_difference<'a>(&'a self, other: &'a Self) -> SymmetricDifference<'a, T, P, H> {
        SymmetricDifference {
            iter: self.difference(other).chain(other.difference(self)),
        }
    }

    #[inline]
    /// Visits the elements that are in both `self` and `other`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let policy = || LazyFixedTTLPolicy::new(Duration::from_secs(30));
    ///
    /// let mut a = HashSet::new(policy());
    /// let mut b = HashSet::new(policy());
    ///
    /// a.insert(1, ());
    /// a.insert(2, ());
    /// b.insert(2, ());
    /// b.insert(3, ());
    ///
    /// let mut both: Vec<_> = a.intersection(&b).copied().collect();
    /// both.sort_unstable();
    ///
    /// assert_eq!(both, [2]);
    /// ```
    pub fn intersection<'a>(&'a self, other: &'a Self) -> Intersection<'a, T, P, H> {
        let (smaller, larger) = if self.len() <= other.len() {
            (self, other)
        } else {
            (other, self)
        };
        Intersection {
            smaller: smaller.iter(),
            larger,
        }
    }

    #[inline]
    /// Visits the elements that are in `self` or in `other`, without repeating the shared ones.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let policy = || LazyFixedTTLPolicy::new(Duration::from_secs(30));
    ///
    /// let mut a = HashSet::new(policy());
    /// let mut b = HashSet::new(policy());
    ///
    /// a.insert(1, ());
    /// a.insert(2, ());
    /// b.insert(2, ());
    /// b.insert(3, ());
    ///
    /// let mut all: Vec<_> = a.union(&b).copied().collect();
    /// all.sort_unstable();
    ///
    /// assert_eq!(all, [1, 2, 3]);
    /// ```
    pub fn union<'a>(&'a self, other: &'a Self) -> Union<'a, T, P, H> {
        let (smaller, larger) = if self.len() <= other.len() {
            (self, other)
        } else {
            (other, self)
        };
        Union {
            iter: larger.iter().chain(smaller.difference(larger)),
        }
    }

    #[inline]
    /// Returns `true` if the set holds the value and it has not expired.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, ());
    ///
    /// assert!(cache.contains(&0));
    /// assert!(!cache.contains(&1));
    ///
    /// sleep(Duration::from_millis(15));
    /// assert!(!cache.contains(&0));
    /// ```
    pub fn contains<Q: ?Sized>(&self, value: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.map.contains_key(value)
    }

    #[inline]
    /// Returns a reference to the value held by the set.
    ///
    /// If the element is expired, returns `None`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, ());
    ///
    /// assert_eq!(cache.get(&0), Some(&0));
    /// assert_eq!(cache.get(&1), None);
    ///
    /// sleep(Duration::from_millis(15));
    /// assert_eq!(cache.get(&0), None);
    /// ```
    pub fn get<Q: ?Sized>(&self, value: &Q) -> Option<&T>
    where
        T: Borrow<Q>,
        Q: Hash + Eq,
    {
        match self.map.get_key_value(value) {
            Some((k, _)) => Some(k),
            None => None,
        }
    }

    #[inline]
    /// Returns a reference to the value in the set, inserting it first if it is absent.
    ///
    /// An expired element counts as absent, so it is inserted again with a fresh `init`. A live
    /// one is handed back untouched -- in particular its expiration state is *not* renewed, so
    /// reading an element never extends its life.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(50)));
    ///
    /// assert_eq!(cache.get_or_insert(0, ()), &0);
    /// assert_eq!(cache.len(), 1);
    ///
    /// // the element is still alive, so it is handed back as it is.
    /// sleep(Duration::from_millis(30));
    /// assert_eq!(cache.get_or_insert(0, ()), &0);
    /// assert_eq!(cache.len(), 1);
    ///
    /// // and the read above did not buy it any extra time.
    /// sleep(Duration::from_millis(30));
    /// assert!(!cache.contains(&0));
    /// ```
    pub fn get_or_insert(&mut self, value: T, init: P::Info) -> &T {
        // Although the raw bucket gives us `&mut T`, we only return `&T` to be consistent with
        // `get`. Value mutation is "raw" because you're not supposed to affect `Eq` or `Hash`.
        let bucket = match self.map.raw_get(&value) {
            Some(bucket) => bucket,
            None => unsafe { self.map.raw_insert(value, (), init).0 },
        };

        // SAFETY: the bucket points into the map's table, and the returned reference borrows
        // `self` for its whole lifetime, so no further mutation can invalidate it.
        unsafe { &bucket.as_ref().0 }
    }

    #[inline]
    /// Returns a reference to the value in the set, inserting an owned copy of `value` first if
    /// it is absent.
    ///
    /// Nothing is cloned when the element is already there.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache: HashSet<String, _, _> =
    ///     HashSet::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    ///
    /// cache.insert(String::from("cat"), ());
    ///
    /// for pet in &["cat", "fish"] {
    ///     assert_eq!(cache.get_or_insert_owned(*pet, ()), pet);
    /// }
    ///
    /// // "cat" was already there, so only "fish" was allocated.
    /// assert_eq!(cache.len(), 2);
    /// ```
    pub fn get_or_insert_owned<Q: ?Sized>(&mut self, value: &Q, init: P::Info) -> &T
    where
        T: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = T>,
    {
        self.get_or_insert_with(value, |value| value.to_owned(), init)
    }

    #[inline]
    /// Returns a reference to the value in the set, inserting `f(value)` first if it is absent.
    ///
    /// `f` is not called when the element is already there.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache: HashSet<String, _, _> =
    ///     HashSet::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    ///
    /// cache.insert(String::from("cat"), ());
    ///
    /// for pet in &["cat", "fish"] {
    ///     assert_eq!(cache.get_or_insert_with(*pet, str::to_owned, ()), pet);
    /// }
    ///
    /// assert_eq!(cache.len(), 2);
    /// ```
    pub fn get_or_insert_with<Q: ?Sized, F>(&mut self, value: &Q, f: F, init: P::Info) -> &T
    where
        T: Borrow<Q>,
        Q: Hash + Eq,
        F: FnOnce(&Q) -> T,
    {
        // `f` is only called on a miss, so a hit never pays for owning the value.
        let bucket = match self.map.raw_get(value) {
            Some(bucket) => bucket,
            None => unsafe { self.map.raw_insert(f(value), (), init).0 },
        };

        // SAFETY: see `get_or_insert`.
        unsafe { &bucket.as_ref().0 }
    }

    /// Returns `true` if `self` shares no live element with `other`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let policy = || LazyFixedTTLPolicy::new(Duration::from_secs(30));
    ///
    /// let mut a = HashSet::new(policy());
    /// let mut b = HashSet::new(policy());
    ///
    /// a.insert(1, ());
    /// b.insert(2, ());
    /// assert!(a.is_disjoint(&b));
    ///
    /// b.insert(1, ());
    /// assert!(!a.is_disjoint(&b));
    /// ```
    pub fn is_disjoint(&self, other: &Self) -> bool {
        self.iter().all(|v| !other.contains(v))
    }

    /// Returns `true` if every live element of `self` is also in `other`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let policy = || LazyFixedTTLPolicy::new(Duration::from_secs(30));
    ///
    /// let mut a = HashSet::new(policy());
    /// let mut b = HashSet::new(policy());
    ///
    /// a.insert(1, ());
    /// b.insert(1, ());
    /// b.insert(2, ());
    ///
    /// assert!(a.is_subset(&b));
    /// assert!(!b.is_subset(&a));
    /// ```
    pub fn is_subset(&self, other: &Self) -> bool {
        self.len() <= other.len() && self.iter().all(|v| other.contains(v))
    }

    #[inline]
    /// Returns `true` if every live element of `other` is also in `self`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let policy = || LazyFixedTTLPolicy::new(Duration::from_secs(30));
    ///
    /// let mut a = HashSet::new(policy());
    /// let mut b = HashSet::new(policy());
    ///
    /// a.insert(1, ());
    /// a.insert(2, ());
    /// b.insert(1, ());
    ///
    /// assert!(a.is_superset(&b));
    /// assert!(!b.is_superset(&a));
    /// ```
    pub fn is_superset(&self, other: &Self) -> bool {
        other.is_subset(self)
    }

    #[inline]
    /// Adds a value to the set, initializing its expiration state from `init`.
    ///
    /// Returns whether the value was newly inserted. An expired element counts as absent, so
    /// re-inserting over one reports `true`.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    ///
    /// assert!(cache.insert(0, ()));
    /// assert!(!cache.insert(0, ()));
    ///
    /// sleep(Duration::from_millis(15));
    /// assert!(cache.insert(0, ()));
    /// ```
    pub fn insert(&mut self, value: T, init: P::Info) -> bool {
        self.map.insert(value, (), init).is_none()
    }

    #[inline]
    /// Adds a value to the set, replacing the existing one, and returns it if there was any.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    ///
    /// assert_eq!(cache.replace(0, ()), None);
    /// assert_eq!(cache.replace(0, ()), Some(0));
    /// assert_eq!(cache.len(), 1);
    /// ```
    pub fn replace(&mut self, value: T, init: P::Info) -> Option<T> {
        let ret = self.map.remove_entry(&value);

        self.map.insert(value, (), init);
        ret.map(|(k, _)| k)
    }

    #[inline]
    /// Removes a value from the set. Returns whether a live value was present.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, ());
    /// cache.insert(1, ());
    ///
    /// assert!(cache.remove(&1));
    ///
    /// sleep(Duration::from_millis(15));
    /// assert!(!cache.remove(&0));
    /// ```
    pub fn remove<Q: ?Sized>(&mut self, value: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.map.remove(value).is_some()
    }

    /// Removes and returns the value held by the set, if it is there and alive.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, ());
    ///
    /// assert_eq!(cache.take(&1), None);
    /// assert_eq!(cache.take(&0), Some(0));
    ///
    /// cache.insert(1, ());
    /// sleep(Duration::from_millis(15));
    /// assert_eq!(cache.take(&1), None);
    /// ```
    pub fn take<Q: ?Sized>(&mut self, value: &Q) -> Option<T>
    where
        T: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.map.remove_entry(value).map(|(k, _)| k)
    }
}

impl<T, P, H> HashSet<T, P, H>
where
    P: ExpirePolicy,
{
    #[inline]
    /// Returns the number of elements the set can hold without reallocating.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let cache: HashSet<u32, _, _> =
    ///     HashSet::with_capacity(20, LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    ///
    /// assert!(cache.capacity() >= 20);
    /// ```
    pub fn capacity(&self) -> usize {
        self.map.capacity()
    }

    /// An iterator visiting every live element in arbitrary order.
    ///
    /// Expired elements are skipped.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// cache.insert(0, ());
    /// cache.insert(1, ());
    ///
    /// let mut values: Vec<_> = cache.iter().copied().collect();
    /// values.sort_unstable();
    /// assert_eq!(values, [0, 1]);
    ///
    /// sleep(Duration::from_millis(15));
    /// assert_eq!(cache.iter().count(), 0);
    /// ```
    pub fn iter(&self) -> Iter<'_, T, P> {
        Iter {
            iter: self.map.keys(),
        }
    }

    /// Returns the exact number of live elements.
    ///
    /// This is accurate but walks the whole table, so it is `O(n)`. See [`len_approx`] for the
    /// cheap answer.
    ///
    /// [`len_approx`]: #method.len_approx
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// assert_eq!(cache.len(), 0);
    ///
    /// cache.insert(0, ());
    /// assert_eq!(cache.len(), 1);
    ///
    /// sleep(Duration::from_millis(15));
    /// assert_eq!(cache.len(), 0);
    /// ```
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns the number of elements in the set.
    ///
    /// This may count elements that have expired but are not yet reaped from the internal
    /// table.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    /// cache.insert(0, ());
    ///
    /// assert_eq!(cache.len_approx(), 1);
    /// ```
    pub fn len_approx(&self) -> usize {
        self.map.len_approx()
    }

    /// Returns `true` if the set holds no live element.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(10)));
    /// assert!(cache.is_empty());
    ///
    /// cache.insert(0, ());
    /// assert!(!cache.is_empty());
    ///
    /// sleep(Duration::from_millis(15));
    /// assert!(cache.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Clears the set, returning every live element as an iterator.
    ///
    /// The set is emptied even if the iterator is dropped before it is exhausted.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    /// cache.insert(0, ());
    /// cache.insert(1, ());
    ///
    /// let mut drained: Vec<_> = cache.drain().collect();
    /// drained.sort_unstable();
    ///
    /// assert_eq!(drained, [0, 1]);
    /// assert!(cache.is_empty());
    /// ```
    pub fn drain(&mut self) -> Drain<'_, T, P> {
        Drain {
            iter: self.map.drain(),
        }
    }

    /// Removes every element from the set.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    /// cache.insert(0, ());
    /// cache.clear();
    ///
    /// assert!(cache.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.map.clear()
    }
}

/// An iterator over the live elements of a [`HashSet`].
///
/// This `struct` is created by the [`iter`] method on [`HashSet`]. See its documentation for more.
///
/// [`iter`]: HashSet::iter
pub struct Iter<'a, T, P>
where
    P: ExpirePolicy,
{
    iter: map::Keys<'a, T, (), P>,
}

impl<'a, T, P> Iterator for Iter<'a, T, P>
where
    P: ExpirePolicy,
{
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

/// A draining iterator over the live elements of a [`HashSet`].
///
/// This `struct` is created by the [`drain`] method on [`HashSet`]. See its documentation for more.
///
/// [`drain`]: HashSet::drain
pub struct Drain<'a, T, P>
where
    P: ExpirePolicy,
{
    iter: map::Drain<'a, T, (), P>,
}

impl<'a, T, P> Iterator for Drain<'a, T, P>
where
    P: ExpirePolicy,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(k, _)| k)
    }
}

/// An iterator over the elements of a [`HashSet`] that are not in another one.
///
/// This `struct` is created by the [`difference`] method on [`HashSet`]. See its documentation for more.
///
/// [`difference`]: HashSet::difference
pub struct Difference<'a, T, P, H>
where
    P: ExpirePolicy,
{
    iter: Iter<'a, T, P>,
    other: &'a HashSet<T, P, H>,
}

impl<'a, T, P, H> Iterator for Difference<'a, T, P, H>
where
    P: ExpirePolicy,
    T: Hash + Eq,
    H: BuildHasher,
{
    type Item = &'a T;

    /// Skipping is a loop, not a recursion -- see [`map::Iter`].
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let elt = self.iter.next()?;

            if !self.other.contains(elt) {
                return Some(elt);
            }
        }
    }
}

/// An iterator over the elements that are in one [`HashSet`] or the other, but not in both.
///
/// This `struct` is created by the [`symmetric_difference`] method on [`HashSet`]. See its documentation for more.
///
/// [`symmetric_difference`]: HashSet::symmetric_difference
pub struct SymmetricDifference<'a, T, P, H>
where
    P: ExpirePolicy,
{
    iter: Chain<Difference<'a, T, P, H>, Difference<'a, T, P, H>>,
}

impl<'a, T, P, H> Iterator for SymmetricDifference<'a, T, P, H>
where
    P: ExpirePolicy,
    T: Hash + Eq,
    H: BuildHasher,
{
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

/// An iterator over the elements that two [`HashSet`]s share.
///
/// This `struct` is created by the [`intersection`] method on [`HashSet`]. See its documentation for more.
///
/// [`intersection`]: HashSet::intersection
pub struct Intersection<'a, T, P, H>
where
    P: ExpirePolicy,
{
    smaller: Iter<'a, T, P>,
    larger: &'a HashSet<T, P, H>,
}

impl<'a, T, P, H> Iterator for Intersection<'a, T, P, H>
where
    P: ExpirePolicy,
    T: Hash + Eq,
    H: BuildHasher,
{
    type Item = &'a T;

    /// Skipping is a loop, not a recursion -- see [`map::Iter`].
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let elt = self.smaller.next()?;

            if self.larger.contains(elt) {
                return Some(elt);
            }
        }
    }
}

/// An iterator over the elements of two [`HashSet`]s, without repeating the shared ones.
///
/// This `struct` is created by the [`union`] method on [`HashSet`]. See its documentation for more.
///
/// [`union`]: HashSet::union
pub struct Union<'a, T, P, H>
where
    P: ExpirePolicy,
{
    iter: Chain<Iter<'a, T, P>, Difference<'a, T, P, H>>,
}

impl<'a, T, P, H> Iterator for Union<'a, T, P, H>
where
    P: ExpirePolicy,
    T: Hash + Eq,
    H: BuildHasher,
{
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl<T, P, H> fmt::Debug for HashSet<T, P, H>
where
    T: fmt::Debug,
    P: ExpirePolicy,
{
    /// Formats the live elements. Expired ones are not shown, and none of them are marked as
    /// accessed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl<T, P> Default for HashSet<T, P, DefaultHashBuilder>
where
    P: ExpirePolicy + Default,
{
    /// Creates an empty `HashSet` driven by the policy's own default.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::TTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// let cache: HashSet<u32, TTLPolicy, _> = HashSet::default();
    ///
    /// assert!(cache.is_empty());
    /// ```
    fn default() -> Self {
        Self::new(P::default())
    }
}

impl<T, P, H> PartialEq for HashSet<T, P, H>
where
    T: Eq + Hash,
    P: ExpirePolicy,
    H: BuildHasher,
{
    /// Two sets are equal when they hold the same live elements. Expired ones are ignored, and
    /// the comparison does not count as an access.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::thread::sleep;
    /// use std::time::Duration;
    ///
    /// let policy = || LazyFixedTTLPolicy::new(Duration::from_millis(10));
    ///
    /// let mut a = HashSet::new(policy());
    /// let mut b = HashSet::new(policy());
    ///
    /// a.insert(0, ());
    /// assert_ne!(a, b);
    ///
    /// b.insert(0, ());
    /// assert_eq!(a, b);
    ///
    /// sleep(Duration::from_millis(15));
    /// assert_eq!(a, b);
    /// assert!(a.is_empty());
    /// ```
    fn eq(&self, other: &Self) -> bool {
        self.map == other.map
    }
}

impl<T, P, H> Eq for HashSet<T, P, H>
where
    T: Eq + Hash,
    P: ExpirePolicy,
    H: BuildHasher,
{
}

impl<T, P, H> Extend<(T, P::Info)> for HashSet<T, P, H>
where
    T: Eq + Hash,
    P: ExpirePolicy,
    H: BuildHasher,
{
    /// Inserts every `(value, init)` the iterator yields.
    ///
    /// The expiration info travels with each element: there is no single policy input that
    /// could stand in for all of them.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::TTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(TTLPolicy::new());
    ///
    /// cache.extend([(0, Duration::from_secs(30)), (1, Duration::from_secs(60))]);
    ///
    /// assert_eq!(cache.len(), 2);
    /// ```
    fn extend<I: IntoIterator<Item = (T, P::Info)>>(&mut self, iter: I) {
        self.map
            .extend(iter.into_iter().map(|(value, init)| (value, (), init)))
    }
}

impl<T, P> FromIterator<(T, P::Info)> for HashSet<T, P, DefaultHashBuilder>
where
    T: Eq + Hash,
    P: ExpirePolicy + Default,
{
    /// Collects `(value, init)` pairs into a `HashSet` driven by the policy's default.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::TTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let cache: HashSet<u32, TTLPolicy, _> =
    ///     vec![(0, Duration::from_secs(30)), (1, Duration::from_secs(60))]
    ///         .into_iter()
    ///         .collect();
    ///
    /// assert!(cache.contains(&0));
    /// ```
    fn from_iter<I: IntoIterator<Item = (T, P::Info)>>(iter: I) -> Self {
        let mut set = Self::default();
        set.extend(iter);
        set
    }
}

impl<'a, T, P, H> IntoIterator for &'a HashSet<T, P, H>
where
    P: ExpirePolicy,
{
    type Item = &'a T;
    type IntoIter = Iter<'a, T, P>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T, P, H> IntoIterator for HashSet<T, P, H>
where
    P: ExpirePolicy,
{
    type Item = T;
    type IntoIter = IntoIter<T, P>;

    /// Consumes the set and yields every live element. Expired ones are dropped, not yielded.
    ///
    /// # Example
    ///
    /// ```
    /// use endorphin::policy::LazyFixedTTLPolicy;
    /// use endorphin::HashSet;
    ///
    /// use std::time::Duration;
    ///
    /// let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
    /// cache.insert(0, ());
    /// cache.insert(1, ());
    ///
    /// let mut values: Vec<_> = cache.into_iter().collect();
    /// values.sort_unstable();
    ///
    /// assert_eq!(values, [0, 1]);
    /// ```
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            iter: self.map.into_iter(),
        }
    }
}

/// An owning iterator over the live elements of a [`HashSet`].
///
/// This `struct` is created by the [`into_iter`] method on [`HashSet`], through [`IntoIterator`].
///
/// [`into_iter`]: IntoIterator::into_iter
pub struct IntoIter<T, P>
where
    P: ExpirePolicy,
{
    iter: map::IntoIter<T, (), P>,
}

impl<T, P> Iterator for IntoIter<T, P>
where
    P: ExpirePolicy,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(value, _)| value)
    }
}

#[cfg(test)]
mod test_set {
    use super::HashSet;
    use crate::policy::{Command, ExpirePolicy, LazyFixedTTLPolicy};
    use crate::EntryId;

    use std::cell::Cell;
    use std::rc::Rc;
    use std::thread::sleep;
    use std::time::Duration;

    /// A policy that expires nothing by default, but records every event it receives so a
    /// test can assert on them directly instead of going through wall-clock time.
    #[derive(Clone, Default)]
    struct SpyPolicy {
        expired: Rc<Cell<bool>>,
        inserts: Rc<Cell<usize>>,
        accesses: Rc<Cell<usize>>,
    }

    impl ExpirePolicy for SpyPolicy {
        type Info = ();
        type Storage = ();

        fn init_storage(&self, _: Self::Info) -> Self::Storage {}

        fn clear(&mut self) {}

        fn is_expired(&self, _: EntryId, _: &Self::Storage) -> bool {
            self.expired.get()
        }

        fn on_access(&self, _: EntryId, _: &Self::Storage) -> Command {
            self.accesses.set(self.accesses.get() + 1);
            Command::Noop
        }

        fn on_insert(&self, _: EntryId, _: &Self::Storage) -> Command {
            self.inserts.set(self.inserts.get() + 1);
            Command::Noop
        }

        fn on_resize(&self) -> Command {
            Command::Noop
        }
    }

    #[test]
    fn test_get_or_insert_on_absent() {
        let spy = SpyPolicy::default();
        let mut set = HashSet::new(spy.clone());

        assert_eq!(set.get_or_insert(0, ()), &0);
        assert_eq!(set.len(), 1);
        assert!(set.contains(&0));
        assert_eq!(spy.inserts.get(), 1);
    }

    #[test]
    fn test_get_or_insert_on_present() {
        let mut set = HashSet::new(SpyPolicy::default());

        set.insert(String::from("alive"), ());

        // an occupied entry is handed back as-is, and nothing is inserted.
        assert_eq!(set.get_or_insert(String::from("alive"), ()), "alive");
        assert_eq!(set.len(), 1);
    }

    /// `get_or_insert` on a live entry must hand the entry back untouched. Re-inserting it
    /// would re-run `init_storage`, silently granting the entry a brand new lifetime.
    #[test]
    fn test_get_or_insert_does_not_reinitialize_live_entry() {
        let spy = SpyPolicy::default();
        let mut set = HashSet::new(spy.clone());

        set.insert(0, ());
        assert_eq!(spy.inserts.get(), 1);

        assert_eq!(set.get_or_insert(0, ()), &0);
        assert_eq!(spy.inserts.get(), 1, "a live entry must not be re-inserted");
        assert_eq!(set.len(), 1);
    }

    /// An expired entry is absent, so it has to be inserted again.
    #[test]
    fn test_get_or_insert_on_expired() {
        let spy = SpyPolicy::default();
        let mut set = HashSet::new(spy.clone());

        set.insert(0, ());
        spy.expired.set(true);
        assert!(!set.contains(&0));

        assert_eq!(set.get_or_insert(0, ()), &0);
        assert_eq!(spy.inserts.get(), 2, "an expired entry must be re-inserted");

        spy.expired.set(false);
        assert!(set.contains(&0));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_get_or_insert_with() {
        let mut set: HashSet<String, _, _> = HashSet::new(SpyPolicy::default());
        let calls = Cell::new(0);

        let mut f = |value: &str| {
            calls.set(calls.get() + 1);
            value.to_owned()
        };

        assert_eq!(set.get_or_insert_with("cat", &mut f, ()), "cat");
        assert_eq!(calls.get(), 1);

        // `f` is not called on a hit, so a hit never pays for owning the value.
        assert_eq!(set.get_or_insert_with("cat", &mut f, ()), "cat");
        assert_eq!(calls.get(), 1);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_get_or_insert_owned() {
        let mut set: HashSet<String, _, _> = HashSet::new(SpyPolicy::default());

        set.insert(String::from("cat"), ());

        assert_eq!(set.get_or_insert_owned("cat", ()), "cat");
        assert_eq!(set.get_or_insert_owned("fish", ()), "fish");
        assert_eq!(set.len(), 2);
    }

    /// Comparing two caches must not count as using them -- see the same test on `HashMap`.
    #[test]
    fn test_eq_does_not_fire_access_events() {
        let spy = SpyPolicy::default();
        let mut a = HashSet::new(spy.clone());
        let mut b = HashSet::new(SpyPolicy::default());

        a.insert(0, ());
        b.insert(0, ());
        spy.accesses.set(0);

        assert_eq!(a, b);
        assert_eq!(spy.accesses.get(), 0);

        // a plain read still does report itself.
        assert!(a.contains(&0));
        assert_eq!(spy.accesses.get(), 1);
    }

    #[test]
    fn test_eq_ignores_expired_elements() {
        let spy = SpyPolicy::default();
        let mut a = HashSet::new(spy.clone());
        let b = HashSet::new(SpyPolicy::default());

        a.insert(0, ());
        assert_ne!(a, b);

        spy.expired.set(true);
        assert_eq!(a, b);
    }

    #[test]
    fn test_into_iter_skips_expired_elements() {
        let spy = SpyPolicy::default();
        let mut set = HashSet::new(spy.clone());

        set.insert(0, ());
        set.insert(1, ());

        let mut values: Vec<_> = (&set).into_iter().copied().collect();
        values.sort_unstable();
        assert_eq!(values, [0, 1]);

        spy.expired.set(true);
        assert_eq!(set.into_iter().count(), 0);
    }

    #[test]
    fn test_extend_and_from_iter() {
        let mut set = HashSet::new(SpyPolicy::default());
        set.extend(vec![(0, ()), (1, ())]);

        assert_eq!(set.len(), 2);
        assert!(set.contains(&1));

        let collected: HashSet<u32, SpyPolicy, _> = vec![(0, ()), (1, ())].into_iter().collect();

        assert_eq!(collected.len(), 2);
    }

    #[test]
    fn test_debug_shows_live_elements_only() {
        let spy = SpyPolicy::default();
        let mut set = HashSet::new(spy.clone());
        set.insert(0, ());

        assert_eq!(format!("{:?}", set), "{0}");

        spy.expired.set(true);
        assert_eq!(format!("{:?}", set), "{}");
    }

    /// End-to-end check against a real clock. Ignored under Miri, which runs far too slowly
    /// for wall-clock deadlines to mean anything.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_get_or_insert_keeps_original_ttl() {
        let mut set = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(50)));

        set.insert(0, ());

        sleep(Duration::from_millis(30));
        assert_eq!(set.get_or_insert(0, ()), &0);

        // had the hit above re-initialized the entry, it would still be alive here.
        sleep(Duration::from_millis(30));
        assert!(!set.contains(&0));
    }
}
