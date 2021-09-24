use std::collections::BTreeMap;

use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::time::{Duration, Instant};

pub struct TTLTracer {
    // We use UnsafeCell instead of RefCell because RefCell has extra bound checks.
    // for safty reason, we also added phantom marker for !Send, !Sync.
    base: UnsafeCell<BTreeMap<Instant, u64>>,
    base_time: Instant,
    __marker_unsend: PhantomData<*const u8>,
}

impl TTLTracer {
    pub fn new() -> Self {
        Self {
            base: Default::default(),
            base_time: Instant::now(),
            __marker_unsend: PhantomData,
        }
    }

    fn base(&self) -> &mut BTreeMap<Instant, u64> {
        unsafe { &mut *self.base.get() }
    }

    pub fn trace_ttl(&self, exp_at: Instant) {
        // target instant is already expired.
        if self.base_time > exp_at {
            return;
        }

        let ttl = exp_at - self.base_time;
        let aligned_exp_at = self.base_time + Duration::from_secs(ttl.as_secs() + 1);

        let base = self.base();
        if let Some(count) = base.get_mut(&aligned_exp_at) {
            *count += 1;
        } else {
            base.insert(aligned_exp_at, 1);
        }

        return;
    }

    pub fn expire_trace(&self, now: Instant) -> u64 {
        let base = self.base();

        let left_over = base.split_off(&now);
        std::mem::replace(base, left_over).values().sum()
    }

    pub fn approx_count(&self, now: Instant) -> u64 {
        self.base().range(..now).map(|(_, v)| v).sum()
    }
}
