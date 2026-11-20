//! Coturn user-quota and total-quota. The user is the peer id inside the
//! REST username, so a credential refresh cannot open another 16 allocations.

use std::{collections::BTreeMap, sync::Mutex};

pub const PER_USER: usize = 16;
pub const TOTAL: usize = 64;

pub struct Quotas {
    users: Mutex<BTreeMap<String, usize>>,
    total: Mutex<usize>,
}

impl Quotas {
    pub fn new() -> Self {
        Self {
            users: Mutex::new(BTreeMap::new()),
            total: Mutex::new(0),
        }
    }

    pub fn try_acquire(&self, user: &str) -> bool {
        if user.is_empty() {
            return false;
        }
        let mut total = self.total.lock().unwrap();
        if *total >= TOTAL {
            return false;
        }
        let mut users = self.users.lock().unwrap();
        let count = users.entry(user.to_owned()).or_default();
        if *count >= PER_USER {
            return false;
        }
        *count += 1;
        *total += 1;
        true
    }

    pub fn release(&self, user: &str) {
        let mut total = self.total.lock().unwrap();
        let mut users = self.users.lock().unwrap();
        let Some(count) = users.get_mut(user) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            users.remove(user);
        }
        *total = total.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixteen_per_peer_and_sixty_four_overall() {
        let quotas = Quotas::new();
        for index in 0..PER_USER {
            assert!(quotas.try_acquire("alpha"), "allocation {index}");
        }
        assert!(!quotas.try_acquire("alpha"));
        for index in 0..(TOTAL - PER_USER) {
            assert!(
                quotas.try_acquire(&format!("peer-{index}")),
                "overall {index}"
            );
        }
        assert!(!quotas.try_acquire("fresh"));
        quotas.release("alpha");
        assert!(quotas.try_acquire("fresh"));
        assert!(!quotas.try_acquire("alpha"));
        quotas.release("alpha");
        assert!(quotas.try_acquire("alpha"));
        assert!(!quotas.try_acquire(""));
    }
}
