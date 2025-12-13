//! Built-in trigger rules for the macOS System Observer
//!
//! This module provides concrete implementations of trigger rules that determine
//! when AI analysis should be invoked based on system events and metrics.

use crate::events::{DiskEvent, LogEvent, MemoryPressure, MessageType, MetricsEvent, Severity};
use crate::triggers::TriggerRule;
use chrono::{Duration, Utc};

/// Trigger rule that activates when error frequency exceeds a threshold within a time window
///
/// This rule counts error and fault messages within a specified time window and triggers
/// when the count exceeds the configured threshold.
pub struct ErrorFrequencyRule {
    /// Maximum number of errors allowed within the time window
    pub threshold: usize,
    /// Time window to count errors within (in seconds)
    pub window_seconds: i64,
    /// Severity level to assign when this rule triggers
    pub severity: Severity,
}

impl ErrorFrequencyRule {
    /// Create a new error frequency rule
    ///
    /// # Arguments
    ///
    /// * `threshold` - Maximum number of errors allowed within the time window
    /// * `window_seconds` - Time window to count errors within (in seconds)
    /// * `severity` - Severity level to assign when this rule triggers
    pub fn new(threshold: usize, window_seconds: i64, severity: Severity) -> Self {
        Self {
            threshold,
            window_seconds,
            severity,
        }
    }

    /// Create a default error frequency rule (5 errors in 60 seconds = Warning)
    pub fn with_defaults() -> Self {
        Self::new(5, 60, Severity::Warning)
    }
}

impl TriggerRule for ErrorFrequencyRule {
    fn evaluate(
        &self,
        log_events: &[LogEvent],
        _metrics_events: &[MetricsEvent],
        _disk_events: &[DiskEvent],
    ) -> bool {
        let cutoff = Utc::now() - Duration::seconds(self.window_seconds);

        let error_count = log_events
            .iter()
            .filter(|event| {
                event.timestamp >= cutoff
                    && (event.message_type == MessageType::Error
                        || event.message_type == MessageType::Fault)
            })
            .count();

        error_count > self.threshold
    }

    fn name(&self) -> &str {
        "ErrorFrequencyRule"
    }

    fn severity(&self) -> Severity {
        self.severity
    }
}

/// Trigger rule that activates when memory pressure reaches or exceeds a threshold
///
/// This rule monitors the most recent memory pressure readings and triggers when
/// the pressure level meets or exceeds the configured threshold.
pub struct MemoryPressureRule {
    /// Minimum memory pressure level that triggers this rule
    pub threshold: MemoryPressure,
    /// Severity level to assign when this rule triggers
    pub severity: Severity,
}

impl MemoryPressureRule {
    /// Create a new memory pressure rule
    ///
    /// # Arguments
    ///
    /// * `threshold` - Minimum memory pressure level that triggers this rule
    /// * `severity` - Severity level to assign when this rule triggers
    pub fn new(threshold: MemoryPressure, severity: Severity) -> Self {
        Self {
            threshold,
            severity,
        }
    }

    /// Create a default memory pressure rule (Warning level = Warning severity)
    pub fn with_defaults() -> Self {
        Self::new(MemoryPressure::Warning, Severity::Warning)
    }

    /// Create a critical memory pressure rule (Critical level = Critical severity)
    pub fn critical() -> Self {
        Self::new(MemoryPressure::Critical, Severity::Critical)
    }
}

impl TriggerRule for MemoryPressureRule {
    fn evaluate(
        &self,
        _log_events: &[LogEvent],
        metrics_events: &[MetricsEvent],
        _disk_events: &[DiskEvent],
    ) -> bool {
        // Check if any recent metrics event shows memory pressure at or above threshold
        metrics_events
            .iter()
            .any(|event| event.memory_pressure >= self.threshold)
    }

    fn name(&self) -> &str {
        "MemoryPressureRule"
    }

    fn severity(&self) -> Severity {
        self.severity
    }
}

/// Trigger rule that activates when crash indicators are detected in log messages
///
/// This rule looks for specific keywords and patterns in log messages that indicate
/// process crashes, kernel panics, or other serious system failures.
pub struct CrashDetectionRule {
    /// Keywords that indicate crashes when found in log messages
    crash_keywords: Vec<String>,
    /// Severity level to assign when this rule triggers
    pub severity: Severity,
}

impl CrashDetectionRule {
    /// Create a new crash detection rule with custom keywords
    ///
    /// # Arguments
    ///
    /// * `crash_keywords` - Keywords that indicate crashes when found in log messages
    /// * `severity` - Severity level to assign when this rule triggers
    pub fn new(crash_keywords: Vec<String>, severity: Severity) -> Self {
        Self {
            crash_keywords,
            severity,
        }
    }

    /// Create a default crash detection rule with common crash indicators
    pub fn with_defaults() -> Self {
        let keywords = vec![
            "crash".to_string(),
            "crashed".to_string(),
            "segmentation fault".to_string(),
            "segfault".to_string(),
            "kernel panic".to_string(),
            "panic".to_string(),
            "abort".to_string(),
            "terminated unexpectedly".to_string(),
            "signal 11".to_string(),
            "signal 9".to_string(),
            "SIGKILL".to_string(),
            "SIGSEGV".to_string(),
            "SIGABRT".to_string(),
            "exception".to_string(),
            "fatal error".to_string(),
        ];
        Self::new(keywords, Severity::Critical)
    }
}

impl TriggerRule for CrashDetectionRule {
    fn evaluate(
        &self,
        log_events: &[LogEvent],
        _metrics_events: &[MetricsEvent],
        _disk_events: &[DiskEvent],
    ) -> bool {
        // Look for crash keywords in error and fault messages
        log_events.iter().any(|event| {
            (event.message_type == MessageType::Error || event.message_type == MessageType::Fault)
                && self.crash_keywords.iter().any(|keyword| {
                    event
                        .message
                        .to_lowercase()
                        .contains(&keyword.to_lowercase())
                })
        })
    }

    fn name(&self) -> &str {
        "CrashDetectionRule"
    }

    fn severity(&self) -> Severity {
        self.severity
    }
}

/// Trigger rule that activates when resource consumption spikes suddenly
///
/// This rule monitors CPU and GPU power consumption and triggers when usage
/// increases significantly within a short time period.
pub struct ResourceSpikeRule {
    /// Minimum CPU power increase (in milliwatts) to trigger
    pub cpu_spike_threshold_mw: f64,
    /// Minimum GPU power increase (in milliwatts) to trigger (if GPU data available)
    pub gpu_spike_threshold_mw: f64,
    /// Time window to compare current vs previous usage (in seconds)
    pub comparison_window_seconds: i64,
    /// Severity level to assign when this rule triggers
    pub severity: Severity,
}

impl ResourceSpikeRule {
    /// Create a new resource spike rule
    ///
    /// # Arguments
    ///
    /// * `cpu_spike_threshold_mw` - Minimum CPU power increase (in milliwatts) to trigger
    /// * `gpu_spike_threshold_mw` - Minimum GPU power increase (in milliwatts) to trigger
    /// * `comparison_window_seconds` - Time window to compare current vs previous usage
    /// * `severity` - Severity level to assign when this rule triggers
    pub fn new(
        cpu_spike_threshold_mw: f64,
        gpu_spike_threshold_mw: f64,
        comparison_window_seconds: i64,
        severity: Severity,
    ) -> Self {
        Self {
            cpu_spike_threshold_mw,
            gpu_spike_threshold_mw,
            comparison_window_seconds,
            severity,
        }
    }

    /// Create a default resource spike rule (1000mW CPU, 2000mW GPU spike in 30 seconds)
    pub fn with_defaults() -> Self {
        Self::new(1000.0, 2000.0, 30, Severity::Warning)
    }
}

impl TriggerRule for ResourceSpikeRule {
    fn evaluate(
        &self,
        _log_events: &[LogEvent],
        metrics_events: &[MetricsEvent],
        _disk_events: &[DiskEvent],
    ) -> bool {
        if metrics_events.len() < 2 {
            return false; // Need at least 2 data points to detect a spike
        }

        let now = Utc::now();
        let comparison_cutoff = now - Duration::seconds(self.comparison_window_seconds);

        // Get recent metrics (within comparison window)
        let recent_metrics: Vec<_> = metrics_events
            .iter()
            .filter(|event| event.timestamp >= comparison_cutoff)
            .collect();

        if recent_metrics.len() < 2 {
            return false; // Need at least 2 recent data points
        }

        // Sort by timestamp to get chronological order
        let mut sorted_metrics = recent_metrics;
        sorted_metrics.sort_by_key(|event| event.timestamp);

        // Find the maximum upward spike within the time window using running minimum approach
        // This ensures we only detect increases, not decreases
        let mut max_cpu_spike: f64 = 0.0;
        let mut max_gpu_spike: f64 = 0.0;

        // Track running minimums to detect upward spikes only
        let mut cpu_running_min = sorted_metrics[0].cpu_power_mw;
        let mut gpu_running_min = sorted_metrics[0].gpu_power_mw;

        for metric in &sorted_metrics[1..] {
            // Check CPU spike: current value vs running minimum
            let cpu_spike = metric.cpu_power_mw - cpu_running_min;
            if cpu_spike > 0.0 {
                max_cpu_spike = max_cpu_spike.max(cpu_spike);
            }
            // Update running minimum (only decreases, preserving lowest seen value)
            cpu_running_min = cpu_running_min.min(metric.cpu_power_mw);

            // Check GPU spike: current value vs running minimum (if GPU data available)
            if let (Some(current_gpu), Some(min_gpu)) = (metric.gpu_power_mw, gpu_running_min) {
                let gpu_spike = current_gpu - min_gpu;
                if gpu_spike > 0.0 {
                    max_gpu_spike = max_gpu_spike.max(gpu_spike);
                }
                // Update GPU running minimum
                gpu_running_min = Some(min_gpu.min(current_gpu));
            } else if let Some(current_gpu) = metric.gpu_power_mw {
                // First GPU reading after None values
                gpu_running_min = Some(current_gpu);
            }
        }

        // Trigger if either CPU or GPU spike exceeds threshold
        max_cpu_spike >= self.cpu_spike_threshold_mw || max_gpu_spike >= self.gpu_spike_threshold_mw
    }

    fn name(&self) -> &str {
        "ResourceSpikeRule"
    }

    fn severity(&self) -> Severity {
        self.severity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_log_event(
        message_type: MessageType,
        message: &str,
        timestamp_offset_seconds: i64,
    ) -> LogEvent {
        LogEvent {
            timestamp: Utc::now() - Duration::seconds(timestamp_offset_seconds),
            message_type,
            subsystem: "com.apple.test".to_string(),
            category: "test".to_string(),
            process: "testd".to_string(),
            process_id: 1234,
            message: message.to_string(),
        }
    }

    fn create_test_metrics_event(
        cpu_power_mw: f64,
        gpu_power_mw: Option<f64>,
        memory_pressure: MemoryPressure,
        timestamp_offset_seconds: i64,
    ) -> MetricsEvent {
        MetricsEvent {
            timestamp: Utc::now() - Duration::seconds(timestamp_offset_seconds),
            cpu_power_mw,
            cpu_usage_percent: (cpu_power_mw / 50.0).min(100.0),
            gpu_power_mw,
            gpu_usage_percent: gpu_power_mw.map(|p| (p / 100.0).min(100.0)),
            memory_pressure,
            memory_used_mb: match memory_pressure {
                MemoryPressure::Normal => 2048.0,
                MemoryPressure::Warning => 6144.0,
                MemoryPressure::Critical => 12288.0,
            },
            energy_impact: cpu_power_mw + gpu_power_mw.unwrap_or(0.0),
        }
    }

    #[test]
    fn test_error_frequency_rule_no_trigger() {
        let rule = ErrorFrequencyRule::new(5, 60, Severity::Warning);

        // Create 3 error events (below threshold of 5)
        let log_events = vec![
            create_test_log_event(MessageType::Error, "Error 1", 10),
            create_test_log_event(MessageType::Error, "Error 2", 20),
            create_test_log_event(MessageType::Fault, "Fault 1", 30),
        ];

        let metrics_events = vec![];

        assert!(!rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_error_frequency_rule_trigger() {
        let rule = ErrorFrequencyRule::new(3, 60, Severity::Warning);

        // Create 4 error events (above threshold of 3)
        let log_events = vec![
            create_test_log_event(MessageType::Error, "Error 1", 10),
            create_test_log_event(MessageType::Error, "Error 2", 20),
            create_test_log_event(MessageType::Fault, "Fault 1", 30),
            create_test_log_event(MessageType::Error, "Error 3", 40),
        ];

        let metrics_events = vec![];

        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_error_frequency_rule_time_window() {
        let rule = ErrorFrequencyRule::new(2, 30, Severity::Warning); // 30 second window

        // Create events: 2 recent (within 30s) + 2 old (outside 30s)
        let log_events = vec![
            create_test_log_event(MessageType::Error, "Recent error 1", 10), // 10s ago
            create_test_log_event(MessageType::Error, "Recent error 2", 20), // 20s ago
            create_test_log_event(MessageType::Error, "Old error 1", 40), // 40s ago (outside window)
            create_test_log_event(MessageType::Error, "Old error 2", 50), // 50s ago (outside window)
        ];

        let metrics_events = vec![];

        // Should not trigger because only 2 errors in the 30s window (threshold is 2, need >2)
        assert!(!rule.evaluate(&log_events, &metrics_events, &[]));

        // Add one more recent error to exceed threshold
        let mut log_events_with_extra = log_events;
        log_events_with_extra.push(create_test_log_event(
            MessageType::Error,
            "Recent error 3",
            15,
        ));

        // Should trigger because 3 errors in 30s window (> threshold of 2)
        assert!(rule.evaluate(&log_events_with_extra, &metrics_events, &[]));
    }

    #[test]
    fn test_memory_pressure_rule_no_trigger() {
        let rule = MemoryPressureRule::new(MemoryPressure::Warning, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(1000.0, Some(500.0), MemoryPressure::Normal, 10),
            create_test_metrics_event(1200.0, Some(600.0), MemoryPressure::Normal, 20),
        ];

        assert!(!rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_memory_pressure_rule_trigger() {
        let rule = MemoryPressureRule::new(MemoryPressure::Warning, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(1000.0, Some(500.0), MemoryPressure::Normal, 30),
            create_test_metrics_event(1500.0, Some(800.0), MemoryPressure::Warning, 20),
            create_test_metrics_event(1200.0, Some(600.0), MemoryPressure::Normal, 10),
        ];

        // Should trigger because one event has Warning level memory pressure
        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_memory_pressure_rule_critical_trigger() {
        let rule = MemoryPressureRule::new(MemoryPressure::Critical, Severity::Critical);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(1000.0, Some(500.0), MemoryPressure::Normal, 30),
            create_test_metrics_event(1500.0, Some(800.0), MemoryPressure::Warning, 20),
            create_test_metrics_event(2000.0, Some(1000.0), MemoryPressure::Critical, 10),
        ];

        // Should trigger because one event has Critical level memory pressure
        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_crash_detection_rule_no_trigger() {
        let rule = CrashDetectionRule::with_defaults();

        let log_events = vec![
            create_test_log_event(MessageType::Info, "Normal operation", 10),
            create_test_log_event(MessageType::Error, "Network timeout", 20),
        ];

        let metrics_events = vec![];

        assert!(!rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_crash_detection_rule_trigger() {
        let rule = CrashDetectionRule::with_defaults();

        let log_events = vec![
            create_test_log_event(MessageType::Info, "Normal operation", 30),
            create_test_log_event(MessageType::Error, "Application crashed unexpectedly", 20),
            create_test_log_event(MessageType::Fault, "Segmentation fault in process", 10),
        ];

        let metrics_events = vec![];

        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_crash_detection_rule_case_insensitive() {
        let rule = CrashDetectionRule::with_defaults();

        let log_events = vec![create_test_log_event(
            MessageType::Error,
            "Process CRASHED due to SEGFAULT",
            10,
        )];

        let metrics_events = vec![];

        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_crash_detection_rule_custom_keywords() {
        let custom_keywords = vec!["custom_error".to_string(), "special_failure".to_string()];
        let rule = CrashDetectionRule::new(custom_keywords, Severity::Warning);

        let log_events = vec![create_test_log_event(
            MessageType::Error,
            "A custom_error occurred",
            10,
        )];

        let metrics_events = vec![];

        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
        assert_eq!(rule.severity(), Severity::Warning);
    }

    #[test]
    fn test_resource_spike_rule_no_trigger_insufficient_data() {
        let rule = ResourceSpikeRule::new(1000.0, 2000.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![create_test_metrics_event(
            1000.0,
            Some(500.0),
            MemoryPressure::Normal,
            10,
        )];

        // Should not trigger with only 1 data point
        assert!(!rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_resource_spike_rule_no_trigger_small_increase() {
        let rule = ResourceSpikeRule::new(1000.0, 2000.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(1000.0, Some(500.0), MemoryPressure::Normal, 25), // Earlier
            create_test_metrics_event(1500.0, Some(800.0), MemoryPressure::Normal, 10), // Later
        ];

        // CPU increase: 1500 - 1000 = 500mW (below 1000mW threshold)
        // GPU increase: 800 - 500 = 300mW (below 2000mW threshold)
        assert!(!rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_resource_spike_rule_trigger_cpu_spike() {
        let rule = ResourceSpikeRule::new(1000.0, 2000.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(1000.0, Some(500.0), MemoryPressure::Normal, 25), // Earlier
            create_test_metrics_event(2500.0, Some(800.0), MemoryPressure::Normal, 10), // Later
        ];

        // CPU increase: 2500 - 1000 = 1500mW (above 1000mW threshold)
        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_resource_spike_rule_trigger_gpu_spike() {
        let rule = ResourceSpikeRule::new(1000.0, 2000.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(1000.0, Some(500.0), MemoryPressure::Normal, 25), // Earlier
            create_test_metrics_event(1200.0, Some(3000.0), MemoryPressure::Normal, 10), // Later
        ];

        // CPU increase: 1200 - 1000 = 200mW (below 1000mW threshold)
        // GPU increase: 3000 - 500 = 2500mW (above 2000mW threshold)
        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_resource_spike_rule_no_gpu_data() {
        let rule = ResourceSpikeRule::new(1000.0, 2000.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(1000.0, None, MemoryPressure::Normal, 25), // Earlier, no GPU
            create_test_metrics_event(2500.0, None, MemoryPressure::Normal, 10), // Later, no GPU
        ];

        // CPU increase: 2500 - 1000 = 1500mW (above 1000mW threshold)
        // GPU data not available, should still trigger on CPU
        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_resource_spike_rule_time_window() {
        let rule = ResourceSpikeRule::new(1000.0, 2000.0, 20, Severity::Warning); // 20 second window

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(1000.0, Some(500.0), MemoryPressure::Normal, 30), // Outside window
            create_test_metrics_event(1200.0, Some(600.0), MemoryPressure::Normal, 15), // In window
            create_test_metrics_event(2500.0, Some(800.0), MemoryPressure::Normal, 5),  // In window
        ];

        // Should compare events within 20s window: 1200 -> 2500 = 1300mW increase (above threshold)
        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_rule_names_and_severities() {
        let error_rule = ErrorFrequencyRule::new(5, 60, Severity::Warning);
        assert_eq!(error_rule.name(), "ErrorFrequencyRule");
        assert_eq!(error_rule.severity(), Severity::Warning);

        let memory_rule = MemoryPressureRule::new(MemoryPressure::Critical, Severity::Critical);
        assert_eq!(memory_rule.name(), "MemoryPressureRule");
        assert_eq!(memory_rule.severity(), Severity::Critical);

        let crash_rule = CrashDetectionRule::with_defaults();
        assert_eq!(crash_rule.name(), "CrashDetectionRule");
        assert_eq!(crash_rule.severity(), Severity::Critical);

        let spike_rule = ResourceSpikeRule::with_defaults();
        assert_eq!(spike_rule.name(), "ResourceSpikeRule");
        assert_eq!(spike_rule.severity(), Severity::Warning);
    }

    #[test]
    fn test_resource_spike_rule_transient_spike() {
        let rule = ResourceSpikeRule::new(1000.0, 2000.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(2000.0, Some(500.0), MemoryPressure::Normal, 25), // t0: baseline
            create_test_metrics_event(6000.0, Some(800.0), MemoryPressure::Normal, 15), // t1: spike up
            create_test_metrics_event(1000.0, Some(600.0), MemoryPressure::Normal, 5), // t2: back down
        ];

        // This should trigger because there was a 4000mW CPU spike from running min (1000) to peak (6000)
        // The running minimum approach correctly detects the upward spike
        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_resource_spike_rule_mixed_up_down_pattern() {
        let rule = ResourceSpikeRule::new(1500.0, 2000.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(3000.0, Some(1000.0), MemoryPressure::Normal, 25), // t0: start high
            create_test_metrics_event(1000.0, Some(800.0), MemoryPressure::Normal, 20), // t1: drop to min
            create_test_metrics_event(4000.0, Some(1200.0), MemoryPressure::Normal, 15), // t2: spike from min
            create_test_metrics_event(2000.0, Some(900.0), MemoryPressure::Normal, 5), // t3: settle
        ];

        // This should trigger because there was a 3000mW CPU spike from min (1000) to peak (4000)
        // Running minimum correctly tracks the lowest point and detects the subsequent spike
        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_resource_spike_rule_no_trigger_on_decrease() {
        let rule = ResourceSpikeRule::new(1000.0, 2000.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(5000.0, Some(4000.0), MemoryPressure::Normal, 25), // t0: high
            create_test_metrics_event(3000.0, Some(2500.0), MemoryPressure::Normal, 15), // t1: decrease
            create_test_metrics_event(1000.0, Some(1000.0), MemoryPressure::Normal, 5), // t2: further decrease
        ];

        // This should NOT trigger because we only have decreases (5000→3000→1000)
        // Even though max-min = 4000mW > threshold, it's a decrease not a spike
        assert!(!rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_resource_spike_rule_transient_gpu_spike() {
        let rule = ResourceSpikeRule::new(1000.0, 1500.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![
            create_test_metrics_event(1000.0, Some(1000.0), MemoryPressure::Normal, 25), // t0: baseline
            create_test_metrics_event(1200.0, Some(4000.0), MemoryPressure::Normal, 15), // t1: GPU spike
            create_test_metrics_event(1100.0, Some(1200.0), MemoryPressure::Normal, 5), // t2: back down
        ];

        // This should trigger because there was a 3000mW GPU spike from t0->t1 (above 1500mW threshold)
        // CPU spike is only 200mW (below 1000mW threshold)
        assert!(rule.evaluate(&log_events, &metrics_events, &[]));
    }

    #[test]
    fn test_default_constructors() {
        let error_rule = ErrorFrequencyRule::with_defaults();
        assert_eq!(error_rule.threshold, 5);
        assert_eq!(error_rule.window_seconds, 60);
        assert_eq!(error_rule.severity(), Severity::Warning);

        let memory_rule = MemoryPressureRule::with_defaults();
        assert_eq!(memory_rule.threshold, MemoryPressure::Warning);
        assert_eq!(memory_rule.severity(), Severity::Warning);

        let memory_critical = MemoryPressureRule::critical();
        assert_eq!(memory_critical.threshold, MemoryPressure::Critical);
        assert_eq!(memory_critical.severity(), Severity::Critical);

        let crash_rule = CrashDetectionRule::with_defaults();
        assert_eq!(crash_rule.severity(), Severity::Critical);

        let spike_rule = ResourceSpikeRule::with_defaults();
        assert_eq!(spike_rule.cpu_spike_threshold_mw, 1000.0);
        assert_eq!(spike_rule.gpu_spike_threshold_mw, 2000.0);
        assert_eq!(spike_rule.comparison_window_seconds, 30);
        assert_eq!(spike_rule.severity(), Severity::Warning);
    }
}
