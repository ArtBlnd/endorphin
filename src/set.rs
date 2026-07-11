use crate::{map, policy::ExpirePolicy, HashMap};

use hashbrown::hash_map::DefaultHashBuilder;

use std::{
    borrow::Borrow,
    hash::{BuildHasher, Hash},
    iter::Chain,
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
    pub fn reserve(&mut self, additional: usize) {
        self.map.reserve(additional)
    }

    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.map.shrink_to_fit()
    }

    #[inline]
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.map.shrink_to(min_capacity)
    }

    #[inline]
    pub fn difference<'a>(&'a self, other: &'a Self) -> Difference<'a, T, P, H> {
        Difference {
            iter: self.iter(),
            other,
        }
    }

    #[inline]
    pub fn symmetric_difference<'a>(&'a self, other: &'a Self) -> SymmetricDifference<'a, T, P, H> {
        SymmetricDifference {
            iter: self.difference(other).chain(other.difference(self)),
        }
    }

    #[inline]
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
    pub fn contains<Q: ?Sized>(&self, value: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.map.contains_key(value)
    }

    #[inline]
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
    pub fn get_or_insert_owned<Q: ?Sized>(&mut self, value: &Q, init: P::Info) -> &T
    where
        T: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = T>,
    {
        self.get_or_insert_with(value, |value| value.to_owned(), init)
    }

    #[inline]
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

    pub fn is_disjoint(&self, other: &Self) -> bool {
        self.iter().all(|v| !other.contains(v))
    }

    pub fn is_subset(&self, other: &Self) -> bool {
        self.len() <= other.len() && self.iter().all(|v| other.contains(v))
    }

    #[inline]
    pub fn is_superset(&self, other: &Self) -> bool {
        other.is_subset(self)
    }

    #[inline]
    pub fn insert(&mut self, value: T, init: P::Info) -> bool {
        self.map.insert(value, (), init).is_none()
    }

    #[inline]
    pub fn replace(&mut self, value: T, init: P::Info) -> Option<T> {
        let ret = self.map.remove_entry(&value);

        self.map.insert(value, (), init);
        ret.map(|(k, _)| k)
    }

    #[inline]
    pub fn remove<Q: ?Sized>(&mut self, value: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.map.remove(value).is_some()
    }

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
    pub fn capacity(&self) -> usize {
        self.map.capacity()
    }

    pub fn iter(&self) -> Iter<'_, T, P> {
        Iter {
            iter: self.map.keys(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn len_approx(&self) -> usize {
        self.map.len_approx()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn drain(&mut self) -> Drain<'_, T, P> {
        Drain {
            iter: self.map.drain(),
        }
    }

    pub fn clear(&mut self) {
        self.map.clear()
    }
}

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

    fn next(&mut self) -> Option<Self::Item> {
        let elt = self.iter.next()?;
        if self.other.contains(elt) {
            self.next()
        } else {
            Some(elt)
        }
    }
}

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

    fn next(&mut self) -> Option<Self::Item> {
        let elt = self.smaller.next()?;
        if self.larger.contains(elt) {
            Some(elt)
        } else {
            self.next()
        }
    }
}

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
