use crate::policy::{Command, ExpirePolicy};
use crate::EntryId;

use std::time::{Duration, Instant};

pub struct LazyFixedTTLPolicy {
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

    fn clear(&mut self) {}

    fn is_expired(&self, _: EntryId, storage: &Self::Storage) -> bool {
        *storage > Instant::now()
    }

    fn on_access(&self, entry: EntryId, storage: &Self::Storage) -> Command {
        if *storage > Instant::now() {
            Command::Remove(entry)
        } else {
            Command::Noop
        }
    }

    fn on_insert(&self, _: EntryId, _: &Self::Storage) -> Command {
        Command::Noop
    }

    fn on_resize(&self) -> Command {
        Command::Noop
    }
}
