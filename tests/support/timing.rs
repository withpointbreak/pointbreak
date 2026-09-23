//! Shared timing for integration-test waits.

use std::time::Duration;

/// Upper bound for a test's wait on progress it expects to happen. Only a real
/// hang should reach it, so a loaded host passes and a deadlock still fails.
pub const HANG_GUARD: Duration = Duration::from_secs(60);

/// Sleep before readiness poll `attempt` (0-based): 20 ms doubling, capped at 250 ms.
pub fn poll_backoff(attempt: u32) -> Duration {
    Duration::from_millis((20_u64 << attempt.min(4)).min(250))
}
