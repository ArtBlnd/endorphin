use crate::instrinsic::*;
use crate::policy::{Command, ExpirePolicy};
use crate::EntryId;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use atomic::{Atomic, Ordering};
use parking_lot::RwLock;

pub struct TTLPolicy {
    ttl_records: RwLock<BTreeMap<Instant, Vec<Option<EntryId>>>>,
    ttl_last_update: Atomic<Instant>,
    presision: Duration,
}

impl TTLPolicy {
    #[must_use]
    pub fn new() -> Self {
        // default presision is 1 seconds
        Self::with_presision(Duration::from_millis(100))
    }

    #[must_use]
    pub fn with_presision(presision: Duration) -> Self {
        Self {
            ttl_records: RwLock::new(BTreeMap::new()),
            ttl_last_update: Atomic::new(Instant::now()),
            presision,
        }
    }
}

impl ExpirePolicy for TTLPolicy {
    type Info = Duration;
    type Storage = Instant;

    fn init_storage(&self, ttl: Self::Info) -> Self::Storage {
        Instant::now() + ttl
    }

    fn clear(&mut self) {
        self.ttl_records.write().clear();
        *self.ttl_last_update.get_mut() = Instant::now();
    }

    fn is_expired(&self, _: EntryId, expire_at: &Self::Storage) -> bool {
        *expire_at < Instant::now()
    }

    fn on_access(&self, _: EntryId, _: &Self::Storage) -> Command {
        let now = Instant::now();
        loop {
            let last_update = self.ttl_last_update.load(Ordering::Relaxed);
            if likely(last_update + self.presision > now) {
                return Command::Noop;
            }

            if self
                .ttl_last_update
                .compare_exchange_weak(last_update, now, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }

        // if target entry did not expired yet...
        let mut records = self.ttl_records.write();
        let expires_at = if let Some(v) = records.keys().next() {
            v.clone()
        } else {
            return Command::Noop;
        };

        // target entry did not expired yet.
        if expires_at > now {
            return Command::Noop;
        }

        Command::RemoveBulk(records.remove(&expires_at).unwrap())
    }

    fn on_insert(&self, entry: EntryId, expire_at: &Self::Storage) -> Command {
        let slot = align_instant(*expire_at, self.presision);
        {
            let mut ttl_records = self.ttl_records.write();
            ttl_records
                .entry(slot)
                .or_insert(Vec::new())
                .push(Some(entry));
        }

        self.on_access(entry, expire_at)
    }

    fn on_resize(&self) -> Command {
        Command::Noop
    }
}

#[inline]
fn align_instant(instant: Instant, by: Duration) -> Instant {
    use once_cell::sync::Lazy;
    static BASE: Lazy<Instant> = Lazy::new(|| Instant::now());

    let v = *BASE;
    if likely(v < instant) {
        let offs = instant - v;
        instant + Duration::from_millis((by.as_millis() - offs.as_millis() % by.as_millis()) as u64)
    } else {
        v
    }
}
