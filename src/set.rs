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
        todo!()
    }

    #[inline]
    pub fn get_or_insert_owned<Q: ?Sized>(&mut self, value: &Q, init: P::Info) -> &T
    where
        T: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = T>,
    {
        todo!()
    }

    #[inline]
    pub fn get_or_insert_with<Q: ?Sized, F>(&mut self, value: &Q, f: F, init: P::Info) -> &T
    where
        T: Borrow<Q>,
        Q: Hash + Eq,
        F: FnOnce(&Q) -> T,
    {
        todo!()
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
        ret.map(|(k, v)| k)
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
