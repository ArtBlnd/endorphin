use crate::EntryId;

use std::ptr::NonNull;

pub(crate) struct Storage<S> {
    pub(crate) storage: S,
    pub(crate) entry_id: EntryId,
}
