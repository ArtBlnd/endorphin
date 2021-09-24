use std::time::Instant;

use rand::random;

pub struct ValueEntry<V> {
    value_uid: u64,
    value_exp: Instant,
    value: Option<V>,
}

impl<V> ValueEntry<V> {
    pub fn new(value: V, exp_at: Instant) -> Self {
        Self {
            value_uid: random(),
            value_exp: exp_at,
            value: Some(value),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.value.is_none() || self.value_exp < Instant::now()
    }

    pub fn get(&self) -> Option<&V> {
        if self.is_expired() {
            None
        } else {
            self.value.as_ref()
        }
    }

    pub fn get_mut(&mut self) -> Option<&mut V> {
        if self.is_expired() {
            None
        } else {
            self.value.as_mut()
        }
    }

    pub fn into_inner(self) -> Option<V> {
        self.value
    }

    pub fn take_inner(&mut self) -> Option<V> {
        self.value.take()
    }
}

impl<V> Eq for ValueEntry<V> {}
impl<V> PartialEq for ValueEntry<V> {
    fn eq(&self, other: &Self) -> bool {
        self.value_uid == other.value_uid
    }
}
