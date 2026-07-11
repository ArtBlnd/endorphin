//! A whole batch of entries expiring at once is the normal state of a TTL cache, and every
//! iterator here has to walk past all of them to answer. These used to recurse once per skipped
//! element, so a fully expired cache overflowed the stack on `iter`, and therefore on `len` and
//! `is_empty` too.

use endorphin::policy::{LazyFixedTTLPolicy, TTLPolicy};
use endorphin::{HashMap, HashSet};

use std::thread::sleep;
use std::time::Duration;

const BATCH: u32 = 200_000;

#[test]
fn map_iteration_over_a_fully_expired_cache() {
    let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_millis(1)));
    for i in 0..BATCH {
        cache.insert(i, i, ());
    }

    sleep(Duration::from_millis(5));

    assert_eq!(cache.iter().next(), None);
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
}

#[test]
fn set_iteration_over_a_fully_expired_cache() {
    let mut cache = HashSet::new(LazyFixedTTLPolicy::new(Duration::from_millis(1)));
    for i in 0..BATCH {
        cache.insert(i, ());
    }

    sleep(Duration::from_millis(5));

    assert_eq!(cache.iter().next(), None);
    assert!(cache.is_empty());
}

/// `Difference` skips every element the other set holds, and `Intersection` skips every element
/// it does not. Either can skip the whole set without yielding once.
#[test]
fn set_operations_skipping_every_element() {
    let mut a = HashSet::new(TTLPolicy::new());
    let mut b = HashSet::new(TTLPolicy::new());

    for i in 0..BATCH {
        a.insert(i, Duration::from_secs(30));
        b.insert(i, Duration::from_secs(30));
    }

    // `a` and `b` are equal, so the difference is empty and every element is skipped.
    assert_eq!(a.difference(&b).next(), None);

    let mut disjoint = HashSet::new(TTLPolicy::new());
    disjoint.insert(BATCH, Duration::from_secs(30));

    // nothing is shared, so the intersection is empty and every element is skipped.
    assert_eq!(a.intersection(&disjoint).next(), None);
}
