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
/// use synctui::logic::performance::should_flush_batch;
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
