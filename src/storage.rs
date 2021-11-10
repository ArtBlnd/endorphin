use crate::EntryId;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

pub struct Storage<S> {
    pub(crate) storage: S,
    pub(crate) entry_id: EntryId,

    is_expired: AtomicBool,
}

impl<S> Storage<S> {
    pub fn new(storage: S, entry_id: EntryId) -> Self {
        Self {
            storage,
            entry_id,
            is_expired: AtomicBool::new(false),
        }
    }

    pub fn mark_as_removed(&self) {
        self.is_expired.store(true, Ordering::Release);
    }

    pub fn is_removed(&self) -> bool {
        self.is_expired.load(Ordering::Acquire)
    }
}
