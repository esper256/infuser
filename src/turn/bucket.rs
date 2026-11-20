//! Per-allocation token bucket. coturn max-bps is 524288 and bps-capacity is 8388608.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

pub const MAX_BYTES_PER_SECOND: f64 = 524_288.0;
pub const BURST_BYTES: f64 = 8_388_608.0;

pub struct Bucket {
    rate: f64,
    capacity: f64,
    state: Mutex<(f64, Instant)>,
}

impl Bucket {
    pub fn new(rate: f64, capacity: f64) -> Self {
        Self {
            rate,
            capacity,
            state: Mutex::new((capacity, Instant::now())),
        }
    }

    pub fn game() -> Self {
        Self::new(MAX_BYTES_PER_SECOND, BURST_BYTES)
    }

    pub fn allow(&self, bytes: usize) -> bool {
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(state.1);
        // Sub-microsecond gaps add nothing; the next packet refills from this instant.
        let refill = duration_secs(elapsed) * self.rate;
        state.0 = (state.0 + refill).min(self.capacity);
        state.1 = now;
        let cost = bytes as f64;
        if state.0 < cost {
            return false;
        }
        state.0 -= cost;
        true
    }
}

fn duration_secs(duration: Duration) -> f64 {
    duration.as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_is_spent_then_refused() {
        let bucket = Bucket::new(0.0, 10.0);
        assert!(bucket.allow(10));
        assert!(!bucket.allow(1));
    }
}
