use crate::policy::{ExpirePolicy, Status};

use std::time::{Duration, Instant};

struct LazyFixedTTLPolicy {
    ttl: Duration,
}

impl LazyFixedTTLPolicy {
    pub fn new(ttl: Duration) -> Self {
        Self { ttl }
    }
}

impl ExpirePolicy for LazyFixedTTLPolicy {
    type Info = ();
    type Storage = Instant;

    fn init_storage(&self, _: Self::Info) -> Self::Storage {
        Instant::now() + self.ttl
    }

    fn clear(&self) {}

    fn on_access(&self, entry: crate::EntryId, storage: &mut Self::Storage) -> Status {
        if *storage > Instant::now() {
            Status::Expired(entry)
        } else {
            Status::Alive
        }
    }

    fn on_insert(&self, _: crate::EntryId, _: &mut Self::Storage) -> Status {
        Status::Alive
    }

    fn on_remove(&self, _: crate::EntryId, _: &mut Self::Storage) -> Status {
        Status::Alive
    }

    fn on_resize(&self) -> Status {
        Status::Alive
    }
}
