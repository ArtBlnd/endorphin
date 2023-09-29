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
//! ```
//! use std::thread::sleep;
//! use std::time::Duration;
//!
//! use endorphin::policy::TTLPolicy;
//! use endorphin::HashMap;
//!
//! fn main() {
//!     let mut cache = HashMap::new(TTLPolicy::new());
//!
//!     cache.insert("Still", "Alive", Duration::from_secs(3));
//!     cache.insert("Gonna", "Die", Duration::from_secs(1));
//!
//!     sleep(Duration::from_secs(1));
//!
//!     assert_eq!(cache.get(&"Still"), Some(&"Alive"));
//!     assert_eq!(cache.get(&"Gonna"), None);
//! }
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

/// A hash map implemented with [`hashbrown`] internal.
/// [`hashbrown`]: https://docs.rs/hashbrown/latest/hashbrown/index.html
pub mod map;

/// A hash set implemented with [`hashbrown`] internal.
/// [`hashbrown`]: https://docs.rs/hashbrown/latest/hashbrown/index.html
pub mod set;

/// An expiration policy including four pre-defined policies.
pub mod policy;

mod entry;

#[doc(inline)]
pub use crate::map::HashMap;

pub use crate::entry::EntryId;
#[doc(inline)]
pub use crate::set::HashSet;

pub use crate::storage::Storage;

pub use entry::*;
