use crate::instrinsic::*;
use crate::policy::{ExpirePolicy, Command};
use crate::{EntryId, ENTRY_TOMBSTONE};

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use parking_lot::RwLockUpgradableReadGuard;

pub struct TTLPolicy {
    ttl_records: RwLock<BTreeMap<Instant, Vec<EntryId>>>,
    ttl_latest_check: RwLock<Instant>,
    presision: Duration,
}

impl TTLPolicy {
    pub fn new(presision: Duration) -> Self {
        Self {
            ttl_records: RwLock::new(BTreeMap::new()),
            ttl_latest_check: RwLock::new(Instant::now()),
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

    fn clear(&self) {
        self.ttl_records.write().clear();
        *self.ttl_latest_check.write() = Instant::now();
    }

    fn is_expired(&self, entry: EntryId, expire_at: &mut Self::Storage) -> bool {
        *expire_at > Instant::now()
    }

    fn on_access(&self, entry: EntryId, expire_at: &mut Self::Storage) -> Command {
        {
            let now = Instant::now();
            let ttl_latest_check = self.ttl_latest_check.upgradable_read();
            if *ttl_latest_check + self.presision < now {
                return Command::Noop;
            }

            *RwLockUpgradableReadGuard::upgrade(ttl_latest_check) = now;
        }

        // if target entry did not expired yet...
        let mut records = self.ttl_records.write();
        let expires_at = if let Some(v) = records.keys().cloned().next() {
            v
        } else {
            return Command::Noop;
        };

        // target entry did not expired yet.
        if expires_at > Instant::now() {
            return Command::Noop;
        }

        Command::RemoveBulk(records.remove(&expires_at).unwrap())
    }

    fn on_insert(&self, entry: EntryId, expire_at: &mut Self::Storage) -> Command {
        let slot = align_instant(*expire_at, self.presision);

        let mut ttl_records = self.ttl_records.write();
        ttl_records.entry(slot).or_insert(Vec::new()).push(entry);

        Command::Noop
    }

    fn on_remove(&self, entry: EntryId, expire_at: &mut Self::Storage) -> Command {
        let slot = align_instant(*expire_at, self.presision);

        let mut ttl_records = self.ttl_records.write();
        for record in ttl_records.get_mut(&slot).unwrap() {
            if *record == entry {
                continue;
            }

            *record = ENTRY_TOMBSTONE;
            return Command::Noop;
        }

        unreachable!();
    }

    fn on_resize(&self) -> Command {
        Command::Noop
    }
}

fn align_instant(instant: Instant, by: Duration) -> Instant {
    use once_cell::sync::Lazy;
    static BASE: Lazy<Instant> = Lazy::new(|| Instant::now());

    let v = *BASE;
    let offs = if likely(v > instant) {
        let offs = v - instant;
        by.as_micros() - (offs.as_micros() % by.as_micros())
    } else {
        let offs = instant - v;
        offs.as_micros() % by.as_micros()
    };

    instant + Duration::from_micros(offs as u64)
}
