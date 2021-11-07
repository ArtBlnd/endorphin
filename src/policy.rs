mod lazy_fixed_ttl_policy;
pub use lazy_fixed_ttl_policy::*;

use crate::EntryId;

pub enum Status {
    // Single entry has been expired.
    Expired(EntryId),
    // Some entry has been expired.
    ExpiredVec(Vec<EntryId>),
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

    fn on_access(&self, entry: EntryId, storage: &mut Self::Storage) -> Status;
    fn on_insert(&self, entry: EntryId, storage: &mut Self::Storage) -> Status;
    fn on_remove(&self, entry: EntryId, storage: &mut Self::Storage) -> Status;
    fn on_resize(&self) -> Status;
}
