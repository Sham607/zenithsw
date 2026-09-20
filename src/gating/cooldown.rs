use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Manages temporary quarantine/cooldown for tokens that fail execution,
/// preventing infinite retry loops and wasting RPC calls.
/// Applies exponential backoff: 15 mins * 2^n, capped at 4 hours (240 mins).
#[derive(Debug, Clone)]
pub struct TokenCooldownRegistry {
    cooldowns: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
    base_mins: u64,
    max_mins: u64,
}

impl Default for TokenCooldownRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenCooldownRegistry {
    pub fn new() -> Self {
        Self::with_params(15, 240)
    }

    pub fn with_params(base_mins: u64, max_mins: u64) -> Self {
        Self {
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
            base_mins: base_mins.max(1),
            max_mins: max_mins.max(base_mins),
        }
    }

    /// Check if a token is currently in cooldown/quarantine.
    /// Returns Some(remaining_seconds) if in active cooldown, None if clear.
    pub fn check_cooldown(&self, token_mint: &str) -> Option<u64> {
        let mut guard = self
            .cooldowns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some((until, _)) = guard.get(token_mint) {
            let now = Instant::now();
            if now < *until {
                return Some((*until - now).as_secs().max(1));
            }
        }
        guard.remove(token_mint);
        None
    }

    /// Record an execution failure for a token, applying exponential backoff:
    /// backoff = min(15 * 2^failure_count minutes, 240 minutes).
    /// Returns the assigned cooldown Duration.
    pub fn record_failure(&self, token_mint: &str) -> Duration {
        let mut guard = self
            .cooldowns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let entry = guard
            .entry(token_mint.to_string())
            .or_insert((Instant::now(), 0));
        let failure_count = entry.1;

        // base_mins * 2^failure_count, capped at max_mins
        let base_mins = self.base_mins.saturating_mul(1u64 << failure_count.min(5));
        let backoff_mins = base_mins.min(self.max_mins);
        let duration = Duration::from_secs(backoff_mins * 60);
        let until = Instant::now() + duration;

        *entry = (until, failure_count.saturating_add(1));
        duration
    }

    /// Explicitly clear cooldown for a token (e.g. upon successful execution).
    pub fn clear_cooldown(&self, token_mint: &str) {
        if let Ok(mut guard) = self.cooldowns.lock() {
            guard.remove(token_mint);
        }
    }

    /// Get current failure count for a token.
    pub fn failure_count(&self, token_mint: &str) -> u32 {
        let guard = self
            .cooldowns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.get(token_mint).map(|(_, count)| *count).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_not_in_cooldown() {
        let registry = TokenCooldownRegistry::new();
        assert_eq!(registry.check_cooldown("TokenA"), None);
        assert_eq!(registry.failure_count("TokenA"), 0);
    }

    #[test]
    fn test_record_failure_exponential_backoff() {
        let registry = TokenCooldownRegistry::new();

        // 1st failure: 15 mins (900s)
        let d1 = registry.record_failure("TokenA");
        assert_eq!(d1, Duration::from_secs(15 * 60));
        assert!(registry.check_cooldown("TokenA").is_some());
        assert_eq!(registry.failure_count("TokenA"), 1);

        // 2nd failure: 30 mins (1800s)
        let d2 = registry.record_failure("TokenA");
        assert_eq!(d2, Duration::from_secs(30 * 60));
        assert_eq!(registry.failure_count("TokenA"), 2);

        // 3rd failure: 60 mins
        let d3 = registry.record_failure("TokenA");
        assert_eq!(d3, Duration::from_secs(60 * 60));

        // 4th failure: 120 mins
        let d4 = registry.record_failure("TokenA");
        assert_eq!(d4, Duration::from_secs(120 * 60));

        // 5th failure: 240 mins (cap)
        let d5 = registry.record_failure("TokenA");
        assert_eq!(d5, Duration::from_secs(240 * 60));

        // 6th failure: still 240 mins (cap)
        let d6 = registry.record_failure("TokenA");
        assert_eq!(d6, Duration::from_secs(240 * 60));
    }

    #[test]
    fn test_clear_cooldown() {
        let registry = TokenCooldownRegistry::new();
        registry.record_failure("TokenB");
        assert!(registry.check_cooldown("TokenB").is_some());

        registry.clear_cooldown("TokenB");
        assert_eq!(registry.check_cooldown("TokenB"), None);
    }
}
