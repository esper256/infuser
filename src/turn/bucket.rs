//! Token buckets. Each allocation may burst about one second at coturn's
//! max-bps (524288). The 8 MiB/s capacity is one server-wide bucket.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

pub const MAX_BYTES_PER_SECOND: f64 = 524_288.0;
pub const GLOBAL_BYTES_PER_SECOND: f64 = 8_388_608.0;

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
        Self::new(MAX_BYTES_PER_SECOND, MAX_BYTES_PER_SECOND)
    }

    pub fn global() -> Self {
        Self::new(GLOBAL_BYTES_PER_SECOND, GLOBAL_BYTES_PER_SECOND)
    }

    pub fn refund(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        state.0 = (state.0 + bytes as f64).min(self.capacity);
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

    #[test]
    fn an_allocation_bursts_one_second_and_the_server_bucket_is_eight_mib() {
        let game = Bucket::game();
        assert!(game.allow(MAX_BYTES_PER_SECOND as usize));
        assert!(!game.allow(1));
        let global = Bucket::global();
        assert!(global.allow(GLOBAL_BYTES_PER_SECOND as usize));
        assert!(!global.allow(1));
        global.refund(1);
        assert!(global.allow(1));
    }
}
