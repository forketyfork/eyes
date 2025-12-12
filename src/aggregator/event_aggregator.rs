//! Event aggregator with rolling buffer implementation
//!
//! This module provides the EventAggregator which stores recent log and metrics events
//! in a time-windowed rolling buffer with capacity limits.

use crate::events::{LogEvent, MetricsEvent};
use chrono::{Duration, Utc};
use std::collections::VecDeque;

/// Event aggregator with rolling buffer storage
///
/// Stores recent log and metrics events with automatic time-based expiration
/// and capacity enforcement. Events older than `max_age` are automatically
/// pruned, and when capacity is reached, the oldest events are removed.
pub struct EventAggregator {
    /// Buffer for log events
    log_buffer: VecDeque<LogEvent>,
    /// Buffer for metrics events
    metrics_buffer: VecDeque<MetricsEvent>,
    /// Maximum age for events before expiration
    max_age: Duration,
    /// Maximum number of events per buffer
    max_size: usize,
}

impl EventAggregator {
    /// Create a new EventAggregator with specified limits
    ///
    /// # Arguments
    ///
    /// * `max_age` - Maximum age for events before they are pruned
    /// * `max_size` - Maximum number of events to store per buffer
    ///
    /// # Examples
    ///
    /// ```
    /// use eyes::aggregator::EventAggregator;
    /// use chrono::Duration;
    ///
    /// let aggregator = EventAggregator::new(Duration::seconds(60), 1000);
    /// ```
    pub fn new(max_age: Duration, max_size: usize) -> Self {
        Self {
            log_buffer: VecDeque::with_capacity(max_size),
            metrics_buffer: VecDeque::with_capacity(max_size),
            max_age,
            max_size,
        }
    }

    /// Add a log event to the buffer
    ///
    /// Automatically prunes old entries and enforces capacity limits.
    ///
    /// # Arguments
    ///
    /// * `event` - The log event to add
    pub fn add_log(&mut self, event: LogEvent) {
        self.log_buffer.push_back(event);
        self.enforce_capacity_logs();
        self.prune_old_entries();
    }

    /// Add a metrics event to the buffer
    ///
    /// Automatically prunes old entries and enforces capacity limits.
    ///
    /// # Arguments
    ///
    /// * `event` - The metrics event to add
    pub fn add_metric(&mut self, event: MetricsEvent) {
        self.metrics_buffer.push_back(event);
        self.enforce_capacity_metrics();
        self.prune_old_entries();
    }

    /// Get recent log events within the specified duration
    ///
    /// Returns references to all log events that occurred within the
    /// specified duration from now.
    ///
    /// # Arguments
    ///
    /// * `duration` - Time window to query (e.g., last 60 seconds)
    ///
    /// # Returns
    ///
    /// Vector of references to log events within the time window
    pub fn get_recent_logs(&self, duration: Duration) -> Vec<&LogEvent> {
        let cutoff = Utc::now() - duration;
        self.log_buffer
            .iter()
            .filter(|event| event.timestamp >= cutoff)
            .collect()
    }

    /// Get recent metrics events within the specified duration
    ///
    /// Returns references to all metrics events that occurred within the
    /// specified duration from now.
    ///
    /// # Arguments
    ///
    /// * `duration` - Time window to query (e.g., last 60 seconds)
    ///
    /// # Returns
    ///
    /// Vector of references to metrics events within the time window
    pub fn get_recent_metrics(&self, duration: Duration) -> Vec<&MetricsEvent> {
        let cutoff = Utc::now() - duration;
        self.metrics_buffer
            .iter()
            .filter(|event| event.timestamp >= cutoff)
            .collect()
    }

    /// Prune old entries from both buffers
    ///
    /// Removes all events older than `max_age` from both log and metrics buffers.
    pub fn prune_old_entries(&mut self) {
        let cutoff = Utc::now() - self.max_age;

        // Remove old log events from the front
        while let Some(event) = self.log_buffer.front() {
            if event.timestamp < cutoff {
                self.log_buffer.pop_front();
            } else {
                break;
            }
        }

        // Remove old metrics events from the front
        while let Some(event) = self.metrics_buffer.front() {
            if event.timestamp < cutoff {
                self.metrics_buffer.pop_front();
            } else {
                break;
            }
        }
    }

    /// Enforce capacity limit for log buffer
    ///
    /// Removes oldest entries if buffer exceeds max_size
    fn enforce_capacity_logs(&mut self) {
        while self.log_buffer.len() > self.max_size {
            self.log_buffer.pop_front();
        }
    }

    /// Enforce capacity limit for metrics buffer
    ///
    /// Removes oldest entries if buffer exceeds max_size
    fn enforce_capacity_metrics(&mut self) {
        while self.metrics_buffer.len() > self.max_size {
            self.metrics_buffer.pop_front();
        }
    }
}

impl Default for EventAggregator {
    fn default() -> Self {
        // Default: 60 second window, 1000 events max
        Self::new(Duration::seconds(60), 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{MemoryPressure, MessageType, Timestamp};

    fn create_test_log_event(timestamp: Timestamp) -> LogEvent {
        LogEvent {
            timestamp,
            message_type: MessageType::Error,
            subsystem: "com.apple.test".to_string(),
            category: "test".to_string(),
            process: "testd".to_string(),
            process_id: 1234,
            message: "Test message".to_string(),
        }
    }

    fn create_test_metrics_event(timestamp: Timestamp) -> MetricsEvent {
        MetricsEvent {
            timestamp,
            cpu_power_mw: 1234.5,
            cpu_usage_percent: 60.0,
            gpu_power_mw: Some(567.8),
            gpu_usage_percent: Some(30.0),
            memory_pressure: MemoryPressure::Normal,
            memory_used_mb: 4096.0,
            energy_impact: 1802.3,
        }
    }

    #[test]
    fn test_add_and_retrieve_logs() {
        let mut aggregator = EventAggregator::new(Duration::seconds(60), 100);
        let now = Utc::now();
        let event = create_test_log_event(now);

        aggregator.add_log(event.clone());

        let recent = aggregator.get_recent_logs(Duration::seconds(60));
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].message, "Test message");
    }

    #[test]
    fn test_add_and_retrieve_metrics() {
        let mut aggregator = EventAggregator::new(Duration::seconds(60), 100);
        let now = Utc::now();
        let event = create_test_metrics_event(now);

        aggregator.add_metric(event.clone());

        let recent = aggregator.get_recent_metrics(Duration::seconds(60));
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].cpu_power_mw, 1234.5);
    }

    #[test]
    fn test_time_based_filtering() {
        let mut aggregator = EventAggregator::new(Duration::seconds(60), 100);
        let now = Utc::now();

        // Add events at different times
        let old_event = create_test_log_event(now - Duration::seconds(70));
        let recent_event = create_test_log_event(now - Duration::seconds(30));

        aggregator.add_log(old_event);
        aggregator.add_log(recent_event);

        // Query for last 60 seconds - should only get the recent one
        let recent = aggregator.get_recent_logs(Duration::seconds(60));
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn test_capacity_enforcement() {
        let mut aggregator = EventAggregator::new(Duration::seconds(60), 5);
        let now = Utc::now();

        // Add 10 events (exceeds capacity of 5)
        for i in 0..10 {
            let event = create_test_log_event(now + Duration::seconds(i));
            aggregator.add_log(event);
        }

        // Should only have 5 events (the most recent ones)
        let all_logs = aggregator.get_recent_logs(Duration::seconds(60));
        assert_eq!(all_logs.len(), 5);
    }

    #[test]
    fn test_prune_old_entries() {
        let mut aggregator = EventAggregator::new(Duration::seconds(60), 100);
        let now = Utc::now();

        // Add old events
        for i in 0..5 {
            let event = create_test_log_event(now - Duration::seconds(70 + i));
            aggregator.add_log(event);
        }

        // Add recent events
        for i in 0..5 {
            let event = create_test_log_event(now - Duration::seconds(30 + i));
            aggregator.add_log(event);
        }

        // Prune should remove old events
        aggregator.prune_old_entries();

        // Should only have recent events
        let all_logs = aggregator.get_recent_logs(Duration::seconds(100));
        assert_eq!(all_logs.len(), 5);
    }
}

// Property-based tests
#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::events::{MemoryPressure, MessageType};
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;

    /// Helper to generate a log event with a specific timestamp offset
    fn create_log_with_offset(offset_seconds: i64) -> LogEvent {
        LogEvent {
            timestamp: Utc::now() - Duration::seconds(offset_seconds),
            message_type: MessageType::Error,
            subsystem: "com.apple.test".to_string(),
            category: "test".to_string(),
            process: "testd".to_string(),
            process_id: 1234,
            message: format!("Event at offset {}", offset_seconds),
        }
    }

    /// Helper to generate a metrics event with a specific timestamp offset
    fn create_metrics_with_offset(offset_seconds: i64) -> MetricsEvent {
        MetricsEvent {
            timestamp: Utc::now() - Duration::seconds(offset_seconds),
            cpu_power_mw: 1234.5,
            cpu_usage_percent: 60.0,
            gpu_power_mw: Some(567.8),
            gpu_usage_percent: Some(30.0),
            memory_pressure: MemoryPressure::Normal,
            memory_used_mb: 4096.0,
            energy_impact: 1802.3,
        }
    }

    /// Generate a sequence of time offsets (in seconds from now)
    /// Ensures offsets are reasonable (0-300 seconds in the past)
    #[derive(Debug, Clone)]
    struct TimeOffsets(Vec<i64>);

    impl Arbitrary for TimeOffsets {
        fn arbitrary(g: &mut Gen) -> Self {
            let size = usize::arbitrary(g) % 50 + 1; // 1-50 events
            let mut offsets = Vec::with_capacity(size);
            for _ in 0..size {
                // Generate offsets between 0 and 300 seconds in the past
                let offset = (u16::arbitrary(g) % 301) as i64;
                offsets.push(offset);
            }
            TimeOffsets(offsets)
        }
    }

    /// Generate a query window duration (in seconds)
    #[derive(Debug, Clone)]
    struct QueryWindow(i64);

    impl Arbitrary for QueryWindow {
        fn arbitrary(g: &mut Gen) -> Self {
            // Generate query windows between 1 and 200 seconds
            let window = (u8::arbitrary(g) % 200 + 1) as i64;
            QueryWindow(window)
        }
    }

    // Feature: macos-system-observer, Property 6: Rolling buffer maintains time-based expiration
    // Validates: Requirements 3.1
    #[quickcheck]
    fn prop_rolling_buffer_time_based_expiration(
        offsets: TimeOffsets,
        query_window: QueryWindow,
    ) -> bool {
        // Create aggregator with large capacity and max_age to not interfere with test
        let mut aggregator = EventAggregator::new(Duration::seconds(400), 1000);

        // Capture reference time BEFORE adding events
        let reference_time = Utc::now();

        // Add log events at various time offsets from reference time
        for offset in &offsets.0 {
            let mut event = create_log_with_offset(*offset);
            event.timestamp = reference_time - Duration::seconds(*offset);
            aggregator.add_log(event);
        }

        // Add metrics events at various time offsets from reference time
        for offset in &offsets.0 {
            let mut event = create_metrics_with_offset(*offset);
            event.timestamp = reference_time - Duration::seconds(*offset);
            aggregator.add_metric(event);
        }

        // Query for events within the time window
        let query_duration = Duration::seconds(query_window.0);

        // Capture query time to calculate expected cutoff
        let query_time = Utc::now();
        let cutoff = query_time - query_duration;

        let recent_logs = aggregator.get_recent_logs(query_duration);
        let recent_metrics = aggregator.get_recent_metrics(query_duration);

        // The key property: ALL returned events must have timestamps >= cutoff
        // We allow a small tolerance for timing jitter (1 second)
        let tolerance = Duration::seconds(1);
        let logs_in_window = recent_logs
            .iter()
            .all(|event| event.timestamp >= cutoff - tolerance);
        let metrics_in_window = recent_metrics
            .iter()
            .all(|event| event.timestamp >= cutoff - tolerance);

        // Additionally verify that no events are from the future
        let logs_not_future = recent_logs
            .iter()
            .all(|event| event.timestamp <= query_time);
        let metrics_not_future = recent_metrics
            .iter()
            .all(|event| event.timestamp <= query_time);

        logs_in_window && metrics_in_window && logs_not_future && metrics_not_future
    }

    /// Generate a buffer capacity (1-100)
    #[derive(Debug, Clone)]
    struct BufferCapacity(usize);

    impl Arbitrary for BufferCapacity {
        fn arbitrary(g: &mut Gen) -> Self {
            // Generate capacities between 1 and 100
            let capacity = (u8::arbitrary(g) % 100 + 1) as usize;
            BufferCapacity(capacity)
        }
    }

    /// Generate a number of events to add (may exceed capacity)
    #[derive(Debug, Clone)]
    struct EventCount(usize);

    impl Arbitrary for EventCount {
        fn arbitrary(g: &mut Gen) -> Self {
            // Generate event counts between 1 and 200
            let count = (u8::arbitrary(g) % 200 + 1) as usize;
            EventCount(count)
        }
    }

    // Feature: macos-system-observer, Property 7: Rolling buffer enforces capacity limits
    // Validates: Requirements 3.2
    #[quickcheck]
    fn prop_rolling_buffer_enforces_capacity_limits(
        capacity: BufferCapacity,
        event_count: EventCount,
    ) -> bool {
        // Create aggregator with specified capacity and large max_age
        let mut aggregator = EventAggregator::new(Duration::seconds(1000), capacity.0);

        let now = Utc::now();

        // Add log events
        for i in 0..event_count.0 {
            let mut event = create_log_with_offset(0);
            // Give each event a unique timestamp to ensure they're all distinct
            event.timestamp = now + Duration::milliseconds(i as i64);
            aggregator.add_log(event);
        }

        // Add metrics events
        for i in 0..event_count.0 {
            let mut event = create_metrics_with_offset(0);
            // Give each event a unique timestamp to ensure they're all distinct
            event.timestamp = now + Duration::milliseconds(i as i64);
            aggregator.add_metric(event);
        }

        // Query for all events (large time window)
        let all_logs = aggregator.get_recent_logs(Duration::seconds(2000));
        let all_metrics = aggregator.get_recent_metrics(Duration::seconds(2000));

        // Property 1: Buffer size should never exceed capacity
        let logs_within_capacity = all_logs.len() <= capacity.0;
        let metrics_within_capacity = all_metrics.len() <= capacity.0;

        // Property 2: If we added more events than capacity, buffer should be at capacity
        let logs_at_capacity = if event_count.0 > capacity.0 {
            all_logs.len() == capacity.0
        } else {
            all_logs.len() == event_count.0
        };

        let metrics_at_capacity = if event_count.0 > capacity.0 {
            all_metrics.len() == capacity.0
        } else {
            all_metrics.len() == event_count.0
        };

        // Property 3: When capacity is exceeded, oldest entries should be removed (FIFO)
        // The most recent events should be retained
        let logs_fifo_correct = if event_count.0 > capacity.0 {
            // The last event added should be in the buffer
            all_logs
                .iter()
                .any(|e| e.timestamp == now + Duration::milliseconds((event_count.0 - 1) as i64))
        } else {
            true // Not applicable when capacity not exceeded
        };

        let metrics_fifo_correct = if event_count.0 > capacity.0 {
            // The last event added should be in the buffer
            all_metrics
                .iter()
                .any(|e| e.timestamp == now + Duration::milliseconds((event_count.0 - 1) as i64))
        } else {
            true // Not applicable when capacity not exceeded
        };

        logs_within_capacity
            && metrics_within_capacity
            && logs_at_capacity
            && metrics_at_capacity
            && logs_fifo_correct
            && metrics_fifo_correct
    }
}
