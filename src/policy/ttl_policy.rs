use crate::instrinsic::*;
use crate::policy::{Command, ExpirePolicy};
use crate::EntryId;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use parking_lot::RwLockUpgradableReadGuard;

pub struct TTLPolicy {
    ttl_records: RwLock<BTreeMap<Instant, Vec<Option<EntryId>>>>,
    ttl_latest_check: RwLock<Instant>,
    presision: Duration,
}

impl TTLPolicy {
    pub fn new() -> Self {
        Self::with_presision(Duration::from_millis(1))
    }

    pub fn with_presision(presision: Duration) -> Self {
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
        *expire_at < Instant::now()
    }

    fn on_access(&self, entry: EntryId, expire_at: &mut Self::Storage) -> Command {
        let now = Instant::now();
        {
            let ttl_latest_check = self.ttl_latest_check.upgradable_read();
            if *ttl_latest_check + self.presision > now {
                return Command::Noop;
            }

            *RwLockUpgradableReadGuard::upgrade(ttl_latest_check) = now;
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

    fn on_insert(&self, entry: EntryId, expire_at: &mut Self::Storage) -> Command {
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

fn align_instant(instant: Instant, by: Duration) -> Instant {
    use once_cell::sync::Lazy;
    static BASE: Lazy<Instant> = Lazy::new(|| Instant::now());

    let v = *BASE;
    let mx = if likely(v > instant) {
        let offs = v - instant;
        offs.as_millis() / by.as_millis() + 1
    } else {
        return v;
    };

    v + Duration::from_millis((by.as_millis() * mx) as u64)
}
