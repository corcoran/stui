//! Performance and batching logic
//!
//! Pure functions for batch processing decisions and performance optimizations.

use std::time::Duration;

/// Check if database writes should be flushed based on batch size or age
///
/// This function implements a batching strategy that balances between:
/// - **Batch size**: Flush when queue reaches MAX_BATCH_SIZE items
/// - **Batch age**: Flush when oldest item exceeds MAX_BATCH_AGE_MS
///
/// This prevents both unbounded queue growth and excessive write latency.
///
/// # Arguments
/// * `queue_len` - Number of pending writes in the queue
/// * `time_since_last_flush` - Duration since last flush operation
///
/// # Returns
/// `true` if writes should be flushed now, `false` otherwise
///
/// # Examples
/// ```
/// use std::time::Duration;
/// use stui::logic::performance::should_flush_batch;
///
/// // Empty queue - no flush needed
/// assert!(!should_flush_batch(0, Duration::from_millis(0)));
///
/// // Queue at threshold - flush needed
/// assert!(should_flush_batch(50, Duration::from_millis(0)));
///
/// // Old batch - flush needed even with small queue
/// assert!(should_flush_batch(10, Duration::from_millis(150)));
/// ```
pub fn should_flush_batch(queue_len: usize, time_since_last_flush: Duration) -> bool {
    const MAX_BATCH_SIZE: usize = 50;
    const MAX_BATCH_AGE_MS: u64 = 100;

    if queue_len == 0 {
        return false;
    }

    queue_len >= MAX_BATCH_SIZE
        || time_since_last_flush > Duration::from_millis(MAX_BATCH_AGE_MS)
}

/// Check if a pending delete operation is stale and should be cleaned up
///
/// Operations older than 60 seconds are considered stale, likely due to:
/// - Network issues
/// - Syncthing being down
/// - User cancelled operation
///
/// # Arguments
/// * `initiated_at` - When the operation started
/// * `now` - Current time
///
/// # Returns
/// `true` if the operation should be removed (stale)
pub fn should_cleanup_stale_pending(initiated_at: std::time::Instant, now: std::time::Instant) -> bool {
    const STALE_TIMEOUT: Duration = Duration::from_secs(60);
    now.duration_since(initiated_at) > STALE_TIMEOUT
}

/// Check if a pending delete operation should be verified for completion
///
/// Verification happens when:
/// 1. Rescan has been triggered (Syncthing notified)
/// 2. At least 5 seconds have passed (buffer for Syncthing to process)
///
/// # Arguments
/// * `initiated_at` - When the operation started
/// * `now` - Current time
/// * `rescan_triggered` - Whether the folder has been rescanned
///
/// # Returns
/// `true` if we should check filesystem to verify deletion completed
pub fn should_verify_pending(
    initiated_at: std::time::Instant,
    now: std::time::Instant,
    rescan_triggered: bool,
) -> bool {
    const BUFFER_TIME: Duration = Duration::from_secs(5);

    rescan_triggered && now.duration_since(initiated_at) >= BUFFER_TIME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_flush_batch_empty_queue() {
        // Empty queue - never flush
        assert!(!should_flush_batch(0, Duration::from_millis(0)));
        assert!(!should_flush_batch(0, Duration::from_millis(500)));
    }

    #[test]
    fn test_should_flush_batch_size_threshold() {
        // At or above MAX_BATCH_SIZE (50) - flush immediately
        assert!(should_flush_batch(50, Duration::from_millis(0)));
        assert!(should_flush_batch(51, Duration::from_millis(0)));
        assert!(should_flush_batch(100, Duration::from_millis(0)));
    }

    #[test]
    fn test_should_flush_batch_below_threshold() {
        // Below MAX_BATCH_SIZE and within time limit - no flush
        assert!(!should_flush_batch(1, Duration::from_millis(0)));
        assert!(!should_flush_batch(10, Duration::from_millis(50)));
        assert!(!should_flush_batch(49, Duration::from_millis(99)));
    }

    #[test]
    fn test_should_flush_batch_age_threshold() {
        // Exceeds MAX_BATCH_AGE_MS (100ms) - flush regardless of size
        assert!(should_flush_batch(1, Duration::from_millis(101)));
        assert!(should_flush_batch(10, Duration::from_millis(150)));
        assert!(should_flush_batch(49, Duration::from_millis(200)));
    }

    #[test]
    fn test_should_flush_batch_exact_age_boundary() {
        // Exactly at age threshold - should NOT flush (> not >=)
        assert!(!should_flush_batch(10, Duration::from_millis(100)));
    }

    #[test]
    fn test_should_flush_batch_both_thresholds() {
        // Both size and age exceeded - definitely flush
        assert!(should_flush_batch(50, Duration::from_millis(101)));
        assert!(should_flush_batch(100, Duration::from_millis(200)));
    }
}

#[cfg(test)]
mod pending_delete_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_should_cleanup_stale_pending_fresh() {
        let initiated_at = Instant::now();
        let now = Instant::now();

        // Fresh (< 60s) should not be cleaned up
        assert!(!should_cleanup_stale_pending(initiated_at, now));
    }

    #[test]
    fn test_should_cleanup_stale_pending_old() {
        let initiated_at = Instant::now() - Duration::from_secs(61);
        let now = Instant::now();

        // Stale (> 60s) should be cleaned up
        assert!(should_cleanup_stale_pending(initiated_at, now));
    }

    #[test]
    fn test_should_cleanup_stale_pending_boundary() {
        let now = Instant::now();
        let initiated_at = now - Duration::from_secs(60);

        // Exactly 60s should not be cleaned up (> not >=)
        assert!(!should_cleanup_stale_pending(initiated_at, now));
    }

    #[test]
    fn test_should_verify_pending_not_ready() {
        let initiated_at = Instant::now();
        let now = Instant::now();
        let rescan_triggered = true;

        // Too recent (< 5s), should not verify yet
        assert!(!should_verify_pending(initiated_at, now, rescan_triggered));
    }

    #[test]
    fn test_should_verify_pending_no_rescan() {
        let initiated_at = Instant::now() - Duration::from_secs(6);
        let now = Instant::now();
        let rescan_triggered = false;

        // Rescan not triggered, should not verify
        assert!(!should_verify_pending(initiated_at, now, rescan_triggered));
    }

    #[test]
    fn test_should_verify_pending_ready() {
        let initiated_at = Instant::now() - Duration::from_secs(6);
        let now = Instant::now();
        let rescan_triggered = true;

        // Rescan triggered and > 5s, should verify
        assert!(should_verify_pending(initiated_at, now, rescan_triggered));
    }
}
