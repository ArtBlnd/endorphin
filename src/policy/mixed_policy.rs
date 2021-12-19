use crate::policy::{Command, ExpirePolicy, TTIPolicy, TTIStorage, TTLPolicy};
use crate::EntryId;

use std::time::{Duration, Instant};

pub enum Expiration {
    TTL(Duration),
    TTI(Duration),
}

pub enum Storage {
    TTL(Instant),
    TTI(TTIStorage),
}

pub struct MixedPolicy {
    ttl: TTLPolicy,
    tti: TTIPolicy,
}

impl MixedPolicy {
    #[must_use]
    pub fn new() -> Self {
        // default presision is 1 seconds
        Self::with_presision(Duration::from_millis(100))
    }

    #[must_use]
    pub fn with_presision(presision: Duration) -> Self {
        Self {
            ttl: TTLPolicy::with_presision(presision),
            tti: TTIPolicy::with_presision(presision),
        }
    }
}

impl ExpirePolicy for MixedPolicy {
    type Info = Expiration;
    type Storage = Storage;

    fn init_storage(&self, exp: Self::Info) -> Self::Storage {
        match exp {
            Expiration::TTL(ttl) => Storage::TTL(self.ttl.init_storage(ttl)),
            Expiration::TTI(tti) => Storage::TTI(self.tti.init_storage(tti)),
        }
    }

    fn clear(&mut self) {
        self.ttl.clear();
        self.tti.clear();
    }

    fn is_expired(&self, entry_id: EntryId, exp: &Self::Storage) -> bool {
        match exp {
            Storage::TTL(ttl) => self.ttl.is_expired(entry_id, ttl),
            Storage::TTI(tti) => self.tti.is_expired(entry_id, tti),
        }
    }

    fn on_access(&self, entry_id: EntryId, exp: &Self::Storage) -> Command {
        match exp {
            Storage::TTL(ttl) => self.ttl.on_access(entry_id, ttl),
            Storage::TTI(tti) => self.tti.on_access(entry_id, tti),
        }
    }

    fn on_insert(&self, entry_id: EntryId, exp: &Self::Storage) -> Command {
        match exp {
            Storage::TTL(ttl) => self.ttl.on_insert(entry_id, ttl),
            Storage::TTI(tti) => self.tti.on_insert(entry_id, tti),
        }
    }

    fn on_resize(&self) -> Command {
        Command::Noop
    }
}
