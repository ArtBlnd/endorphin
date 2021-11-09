mod lazy_fixed_ttl_policy;
pub use lazy_fixed_ttl_policy::*;
mod ttl_policy;
pub use ttl_policy::*;

use crate::EntryId;

pub enum Status {
    // Single entry has been expired.
    Remove(EntryId),
    // Some entry has been expired.
    RemoveBulk(Vec<EntryId>),
    // Everything is good. Seems all entry is alive!
    Alive,
    // Try shrink the table.
    TryShrink,
}

pub trait ExpirePolicy {
    type Info;
    type Storage;

    fn init_storage(&self, info: Self::Info) -> Self::Storage;

    fn clear(&self);

    fn is_expired(&self, entry: EntryId, storage: &mut Self::Storage) -> bool;

    fn on_access(&self, entry: EntryId, storage: &mut Self::Storage) -> Status;
    fn on_insert(&self, entry: EntryId, storage: &mut Self::Storage) -> Status;
    fn on_remove(&self, entry: EntryId, storage: &mut Self::Storage) -> Status;
    fn on_resize(&self) -> Status;
}
