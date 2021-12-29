use crate::instrinsic::*;
use crate::policy::{Command, ExpirePolicy};
use crate::EntryId;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use parking_lot::{RwLock, RwLockUpgradableReadGuard};

pub struct TTLPolicy {
    ttl_records: RwLock<BTreeMap<Instant, Vec<Option<EntryId>>>>,
    ttl_last_update: RwLock<Instant>,
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
            ttl_last_update: RwLock::new(Instant::now()),
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
        if self.ttl_records.read().is_empty() {
            return Command::Noop;
        }

        let now = Instant::now();
        let last_update = self.ttl_last_update.upgradable_read();
        
        if likely(*last_update + self.presision > now) {
            return Command::Noop;
        }

        let mut last_update = match RwLockUpgradableReadGuard::try_upgrade(last_update) {
            Ok(v) => v,
            Err(_) => return Command::Noop,
        };

        *last_update = now;

        // if target entry did not expired yet...
        let mut records = self.ttl_records.write();
        let mut expired_values = Vec::new();
        for _ in 0..16 {
            let expires_at = if let Some(k) = records.keys().next() {
                k.clone()
            } else {
                break;
            };

            if expires_at > now {
                break;
            }

            let v = records.remove(&expires_at).unwrap();
            expired_values.extend_from_slice(&v);
        }

        if expired_values.is_empty() {
            Command::Noop
        } else {
            Command::RemoveBulk(expired_values)
        }
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
