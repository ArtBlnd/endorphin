use crate::instrinsic::*;
use crate::policy::{Command, ExpirePolicy};
use crate::EntryId;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock, RwLockUpgradableReadGuard};

#[derive(Clone)]
pub struct TTIStorage {
    timestamp: Arc<Mutex<Instant>>,
    tti: Duration,
}

impl TTIStorage {
    fn new(tti: Duration) -> Self {
        Self {
            timestamp: Arc::new(Mutex::new(Instant::now() + tti)),
            tti,
        }
    }

    fn is_expired(&self) -> bool {
        let now = Instant::now();
        let nxt = now + self.tti;

        let mut timestamp = self.timestamp.lock();
        if *timestamp > now {
            *timestamp = nxt;
            return false;
        }

        true
    }
}

pub struct TTIPolicy {
    tti_records: RwLock<BTreeMap<Instant, Vec<Option<(EntryId, TTIStorage)>>>>,
    tti_last_update: RwLock<Instant>,
    presision: Duration,
}

impl TTIPolicy {
    #[must_use]
    pub fn new() -> Self {
        // default presision is 1 seconds
        Self::with_presision(Duration::from_millis(100))
    }

    #[must_use]
    pub fn with_presision(presision: Duration) -> Self {
        Self {
            tti_records: RwLock::new(BTreeMap::new()),
            tti_last_update: RwLock::new(Instant::now()),
            presision,
        }
    }
}

impl Default for TTIPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpirePolicy for TTIPolicy {
    type Info = Duration;
    type Storage = TTIStorage;

    fn init_storage(&self, tti: Self::Info) -> Self::Storage {
        TTIStorage::new(tti)
    }

    fn clear(&mut self) {
        self.tti_records.write().clear();
        *self.tti_last_update.get_mut() = Instant::now();
    }

    fn is_expired(&self, _: EntryId, storage: &Self::Storage) -> bool {
        storage.is_expired()
    }

    fn on_access(&self, _: EntryId, _: &Self::Storage) -> Command {
        let now = Instant::now();
        let last_update = self.tti_last_update.upgradable_read();

        if likely(*last_update + self.presision > now) {
            return Command::Noop;
        }

        let mut last_update = match RwLockUpgradableReadGuard::try_upgrade(last_update) {
            Ok(v) => v,
            Err(_) => return Command::Noop,
        };

        *last_update = now;

        // if target entry did not expired yet...
        let mut records = self.tti_records.write();

        let mut expired = Vec::new();

        for _ in 0..(records.len() >> 5 | 1) {
            let expires_at = if let Some(v) = records.keys().next() {
                *v
            } else {
                break;
            };

            // target entry did not expired yet.
            if expires_at > now {
                break;
            }

            for record in records.remove(&expires_at).unwrap() {
                if let Some((entry, storage)) = record {
                    if storage.is_expired() {
                        expired.push(Some(entry));
                    } else {
                        // re-insert to records.
                        let new_slot = align_instant(*storage.timestamp.lock(), self.presision);
                        records
                            .entry(new_slot)
                            .or_insert(Vec::new())
                            .push(Some((entry, storage)));
                    }
                }
            }
        }

        if expired.is_empty() {
            Command::Noop
        } else {
            Command::RemoveBulk(expired)
        }
    }

    fn on_insert(&self, entry: EntryId, storage: &Self::Storage) -> Command {
        let slot = align_instant(*storage.timestamp.lock(), self.presision);
        {
            let mut ttl_records = self.tti_records.write();
            ttl_records
                .entry(slot)
                .or_insert(Vec::new())
                .push(Some((entry, storage.clone())));
        }

        self.on_access(entry, storage)
    }

    fn on_resize(&self) -> Command {
        Command::Noop
    }
}

#[inline]
fn align_instant(instant: Instant, by: Duration) -> Instant {
    use once_cell::sync::Lazy;
    static BASE: Lazy<Instant> = Lazy::new(Instant::now);

    let v = *BASE;
    if likely(v < instant) {
        let offs = instant - v;
        instant + Duration::from_millis((by.as_millis() - offs.as_millis() % by.as_millis()) as u64)
    } else {
        v
    }
}
