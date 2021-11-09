use hashbrown::raw::Bucket;

use ringbuffer::AllocRingBuffer as RingBuffer;
use ringbuffer::{RingBufferExt, RingBufferRead, RingBufferWrite};

use std::mem;

pub type EntryId = usize;
pub static ENTRY_TOMBSTONE: EntryId = 0xFFFFFFFF;

pub(crate) struct BucketIdTable<B> {
    table: Box<[Option<Bucket<B>>]>,
    index_queue: RingBuffer<EntryId>,
}

impl<B> BucketIdTable<B> {
    pub fn with_capacity(capacity: usize) -> Self {
        let mut table = Self {
            // TODO: initialize as uninit.
            table: vec![None; capacity].into_boxed_slice(),
            index_queue: RingBuffer::with_capacity(capacity),
        };

        for i in 0..capacity {
            table.index_queue.push(i as EntryId);
        }

        return table;
    }

    pub fn release_id(&mut self, id: EntryId) -> Option<Bucket<B>> {
        self.index_queue.push(id);
        mem::replace(&mut self.table[id as usize], None)
    }

    pub fn acquire_id(&mut self) -> EntryId {
        let id = unsafe { self.index_queue.dequeue().unwrap() };
        id
    }

    pub fn set_bucket(&mut self, id: EntryId, bucket: Bucket<B>) {}

    pub fn release_with_bucket(&mut self, bucket: Bucket<B>) {
        for item in self.table.iter_mut().filter(|v| v.is_none()) {
            if let Some(item_bucket) = item {
                if item_bucket.as_ptr() == bucket.as_ptr() {
                    *item = None;
                    break;
                }
            }
        }

        // trying to remove unregistered bucket???
        unreachable!();
    }

    pub fn get(&self, id: EntryId) -> Bucket<B> {
        self.table[id].clone().unwrap()
    }

    pub fn clear_table_and_resize(&mut self, capacity: usize) {
        // TODO!!!
        self.table = vec![None; capacity].into_boxed_slice()
    }

    pub fn clear(&mut self) {
        self.clear_table_and_resize(self.table.len());
        self.index_queue.clear();
    }
}
