use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// # Atomic Tick Helper
///
/// invalidating caches every method calls are not efficient. Tick object is a helper that
/// not to try invalidating caches every get or insert calls.
/// `tick()` method on the `Tick` object will return true every `N` ticks so that who triggers
/// 'N'th tick will check for the invalidates.
#[derive(Clone)]
pub(crate) struct Tick<const N: u32> {
    tick_count: Arc<AtomicU32>,
}

impl<const N: u32> Tick<N> {
    pub fn new() -> Self {
        Self {
            tick_count: Arc::new(1.into()),
        }
    }

    pub fn tick(&self) -> bool {
        let ticks = &self.tick_count;

        loop {
            let cur = ticks.load(Ordering::Acquire);
            if (cur % N) == 0 {
                if !ticks
                    .compare_exchange(cur, cur + 1, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    continue;
                }

                return true;
            } else {
                if !ticks
                    .compare_exchange(cur, cur + 1, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    continue;
                }

                return false;
            }
        }
    }

    pub fn clear(&self) {
        self.tick_count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |_| Some(1))
            .unwrap();
    }
}
