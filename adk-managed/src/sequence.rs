use std::sync::atomic::{AtomicU64, Ordering};

/// Per-session monotonic sequence counter.
///
/// Each session maintains its own counter. Every emitted `SessionEvent`
/// gets a unique, strictly increasing `seq` value from this counter.
/// Thread-safe via `AtomicU64`.
///
/// # Example
///
/// ```rust
/// use adk_managed::sequence::SequenceCounter;
///
/// let counter = SequenceCounter::default();
/// assert_eq!(counter.next(), 0);
/// assert_eq!(counter.next(), 1);
/// assert_eq!(counter.next(), 2);
/// ```
pub struct SequenceCounter {
    value: AtomicU64,
}

impl SequenceCounter {
    /// Create a new counter starting at the given value.
    pub fn new(start: u64) -> Self {
        Self { value: AtomicU64::new(start) }
    }

    /// Get the next sequence number (strictly increasing).
    /// First call returns `start`, second returns `start + 1`, etc.
    pub fn next(&self) -> u64 {
        self.value.fetch_add(1, Ordering::SeqCst)
    }

    /// Get the current value without incrementing.
    pub fn current(&self) -> u64 {
        self.value.load(Ordering::SeqCst)
    }
}

impl Default for SequenceCounter {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_starts_at_zero() {
        let counter = SequenceCounter::default();
        assert_eq!(counter.next(), 0);
    }

    #[test]
    fn test_starts_at_custom_value() {
        let counter = SequenceCounter::new(42);
        assert_eq!(counter.next(), 42);
        assert_eq!(counter.next(), 43);
    }

    #[test]
    fn test_increments_monotonically() {
        let counter = SequenceCounter::default();
        let mut prev = counter.next();
        for _ in 0..100 {
            let curr = counter.next();
            assert!(curr > prev, "expected {curr} > {prev}");
            prev = curr;
        }
    }

    #[test]
    fn test_current_does_not_increment() {
        let counter = SequenceCounter::default();
        assert_eq!(counter.current(), 0);
        assert_eq!(counter.current(), 0);
        counter.next();
        assert_eq!(counter.current(), 1);
        assert_eq!(counter.current(), 1);
    }

    #[test]
    fn test_thread_safe_concurrent_access() {
        let counter = Arc::new(SequenceCounter::default());
        let num_threads = 8;
        let increments_per_thread = 1000;

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let counter = Arc::clone(&counter);
                thread::spawn(move || {
                    let mut values = Vec::with_capacity(increments_per_thread);
                    for _ in 0..increments_per_thread {
                        values.push(counter.next());
                    }
                    values
                })
            })
            .collect();

        let mut all_values: Vec<u64> =
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect();

        // All values should be unique (no duplicates)
        all_values.sort();
        all_values.dedup();
        let expected_total = num_threads * increments_per_thread;
        assert_eq!(
            all_values.len(),
            expected_total,
            "expected {expected_total} unique values, got {}",
            all_values.len()
        );

        // Final counter value should equal total increments
        assert_eq!(counter.current(), expected_total as u64);
    }

    #[test]
    fn test_thread_safe_values_are_monotonic_per_thread() {
        let counter = Arc::new(SequenceCounter::default());
        let num_threads = 4;
        let increments_per_thread = 500;

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let counter = Arc::clone(&counter);
                thread::spawn(move || {
                    let mut values = Vec::with_capacity(increments_per_thread);
                    for _ in 0..increments_per_thread {
                        values.push(counter.next());
                    }
                    values
                })
            })
            .collect();

        for handle in handles {
            let values = handle.join().unwrap();
            // Each thread's own sequence should be strictly increasing
            for window in values.windows(2) {
                assert!(
                    window[1] > window[0],
                    "expected monotonically increasing within thread, got {} followed by {}",
                    window[0],
                    window[1]
                );
            }
        }
    }
}
