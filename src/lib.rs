//! Endorphin is key-value based in-memory cache library with custom expiration policies.
//! 
//! Basically, endorphin provides four pre-defined policies.
//! - [`LazyFixedTTLPolicy`] : A Lazy TTL Policy that checks TTL when accessing values.
//! - [`MixedPolicy`] : TTL & TTI mixed policy.
//! - [`TTIPolicy`] : Time To Idle Policy.
//! - [`TTLPolicy`] : Time To Live policy .
//! 
//! You can also define new custom policies by using [`ExpirePolicy`].
//! 
//! # Examples
//! ```no_run
//! use endorphin::HashMap;
//! use endorphin::LazyFixedTTLPolicy;
//! 
//! use std::time::Duration;
//! use std::thread::sleep;
//! 
//! let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
//! cache.insert("expired_after", "30 seconds!", ());
//! 
//! assert_eq!(cache.get("expired_after"), Some(&"30 seconds!"))
//! sleep(Duration::from_secs(30));
//! assert_eq!(cache.get("expired_after").is_none(), true)
//! ```
//! For more examples, visit [here]
//! 
//! [here]: https://github.com/ArtBlnd/endorphin
//! 
//! [`ExpirePolicy`]: policy::ExpirePolicy
//! [`LazyFixedTTLPolicy`]: policy::LazyFixedTTLPolicy
//! [`MixedPolicy`]: policy::MixedPolicy
//! [`TTIPolicy`]: policy::TTIPolicy
//! [`TTLPolicy`]: policy::TTLPolicy

// for internal use.
pub(crate) mod hash;
pub(crate) mod instrinsic;
pub(crate) mod storage;

// for external use.


/// A hash map implemented with [Standard HashMap]-like interfaces.
/// 
/// [Standard HashMap]: std::collections::HashMap
pub mod map;


/// A hash set implemented with [Standard HashSet]-like interfaces.
///
/// [Standard HashSet]: std::collections::HashSet
pub mod set;

/// An expiration policy including four pre-defined policies.
pub mod policy;

mod entry;

#[doc(inline)]
pub use crate::map::HashMap;

#[doc(inline)]
//pub use crate::set::HashSet; <-- not implemented

pub use crate::entry::EntryId;

pub use crate::storage::Storage;

pub use entry::*;


