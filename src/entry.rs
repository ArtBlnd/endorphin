use hashbrown::raw::Bucket;

use slotmap::new_key_type;
use slotmap::SlotMap;

new_key_type! {
    pub struct EntryId;
}

pub(crate) struct EntryIdTable<B> {
    table: SlotMap<EntryId, Option<Bucket<B>>>,
}

impl<B> EntryIdTable<B> {
    pub fn new() -> Self {
        Self {
            table: SlotMap::with_key()
        }
    }

    pub fn release_slot(&mut self, id: EntryId) -> Option<Bucket<B>> {
        self.table.remove(id).unwrap()
    }

    pub fn acquire_slot(&mut self) -> EntryId {
        self.table.insert(None)
    }

    pub fn set_bucket(&mut self, slot: EntryId, bucket: Option<Bucket<B>>) {
        if bucket.is_none() {
            println!("{:?}", slot);
        }

        *self.table.get_mut(slot).unwrap() = bucket;
    }

    pub fn get(&self, id: EntryId) -> Option<Bucket<B>> {
        self.table.get(id).cloned().unwrap()
    }

    pub fn clear(&mut self) {
        self.table.clear()
    }
}
