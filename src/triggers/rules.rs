//! Built-in trigger rules for the macOS System Observer
//!
//! This module provides concrete implementations of trigger rules that determine
//! when AI analysis should be invoked based on system events and metrics.

use crate::events::{
    DiskEvent, LogEvent, MeasurementKind, MemoryPressure, MessageType, MetricsEvent, Severity,
};
use crate::triggers::{RelevantLogGroup, TriggerRule};
use chrono::{Duration, Utc};
use std::collections::{BTreeMap, HashMap, HashSet};

type ErrorSource = (String, String, Option<String>);

fn error_source(event: &LogEvent) -> ErrorSource {
    (
        event.process.clone(),
        event.subsystem.clone(),
        event.broker_client_identity(),
    )
}

fn error_source_name(process: String, subsystem: String, client: Option<String>) -> String {
    let daemon = if subsystem.is_empty() {
        process
    } else {
        format!("{}/{}", subsystem, process)
    };

    match client {
        Some(client) => format!("{} client {}", daemon, client),
        None => daemon,
    }
}

/// Trigger rule that activates when error frequency exceeds a threshold within a time window
///
/// This rule detects either many distinct error/fault signatures or a sudden rate increase for
/// one repeated signature within a specified time window.
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

    fn triggering_sources(&self, log_events: &[LogEvent]) -> HashSet<ErrorSource> {
        let now = Utc::now();
        let cutoff = now - Duration::seconds(self.window_seconds);
        let baseline_cutoff = cutoff - Duration::seconds(self.window_seconds);
        let mut current_counts =
            HashMap::<ErrorSource, HashMap<(MessageType, String), usize>>::new();
        let mut baseline_counts =
            HashMap::<ErrorSource, HashMap<(MessageType, String), usize>>::new();

        for event in log_events.iter().filter(|event| {
            event.timestamp >= baseline_cutoff
                && matches!(event.message_type, MessageType::Error | MessageType::Fault)
                && !event.is_known_benign_noise()
        }) {
            let source = error_source(event);
            let signature = (event.message_type, event.message.clone());
            let counts = if event.timestamp >= cutoff {
                &mut current_counts
            } else {
                &mut baseline_counts
            };
            *counts
                .entry(source)
                .or_default()
                .entry(signature)
                .or_insert(0) += 1;
        }

        current_counts
            .iter()
            .filter_map(|(source, signatures)| {
                if signatures.len() > self.threshold {
                    return Some(source.clone());
                }

                let baseline = baseline_counts.get(source);
                let rate_spiked = signatures.iter().any(|(signature, current_count)| {
                    let baseline_count = baseline
                        .and_then(|counts| counts.get(signature))
                        .copied()
                        .unwrap_or(0);
                    *current_count > self.threshold
                        && *current_count >= baseline_count.saturating_mul(2)
                });
                rate_spiked.then(|| source.clone())
            })
            .collect()
    }
}

impl TriggerRule for ErrorFrequencyRule {
    fn evaluate(
        &self,
        log_events: &[LogEvent],
        _metrics_events: &[MetricsEvent],
        _disk_events: &[DiskEvent],
    ) -> bool {
        !self.triggering_sources(log_events).is_empty()
    }

    fn name(&self) -> &str {
        "ErrorFrequencyRule"
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn relevant_logs<'a>(&self, log_events: &'a [LogEvent]) -> Vec<&'a LogEvent> {
        let cutoff = Utc::now() - Duration::seconds(self.window_seconds);
        let triggering_sources = self.triggering_sources(log_events);
        log_events
            .iter()
            .filter(|event| {
                event.timestamp >= cutoff
                    && matches!(event.message_type, MessageType::Error | MessageType::Fault)
                    && !event.is_known_benign_noise()
                    && triggering_sources.contains(&error_source(event))
            })
            .collect()
    }

    fn relevant_log_groups<'a>(&self, log_events: &'a [LogEvent]) -> Vec<RelevantLogGroup<'a>> {
        let cutoff = Utc::now() - Duration::seconds(self.window_seconds);
        let triggering_sources = self.triggering_sources(log_events);
        let mut groups = BTreeMap::<ErrorSource, Vec<&LogEvent>>::new();

        for event in log_events.iter().filter(|event| {
            event.timestamp >= cutoff
                && matches!(event.message_type, MessageType::Error | MessageType::Fault)
                && !event.is_known_benign_noise()
                && triggering_sources.contains(&error_source(event))
        }) {
            groups.entry(error_source(event)).or_default().push(event);
        }

        groups
            .into_iter()
            .map(|((process, subsystem, client), events)| RelevantLogGroup {
                source: Some(error_source_name(process, subsystem, client)),
                events,
            })
            .collect()
    }

    fn relevant_metrics<'a>(&self, _metrics_events: &'a [MetricsEvent]) -> Vec<&'a MetricsEvent> {
        Vec::new()
    }

    fn relevant_disk_events<'a>(&self, _disk_events: &'a [DiskEvent]) -> Vec<&'a DiskEvent> {
        Vec::new()
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
        metrics_events.iter().any(|event| {
            event.provenance.memory_pressure == MeasurementKind::Measured
                && event.memory_pressure >= self.threshold
        })
    }

    fn name(&self) -> &str {
        "MemoryPressureRule"
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn severity_for(
        &self,
        _log_events: &[LogEvent],
        metrics_events: &[MetricsEvent],
        _disk_events: &[DiskEvent],
    ) -> Severity {
        match metrics_events
            .iter()
            .filter(|event| event.provenance.memory_pressure == MeasurementKind::Measured)
            .map(|event| event.memory_pressure)
            .max()
        {
            Some(MemoryPressure::Critical) => Severity::Critical,
            Some(MemoryPressure::Warning) => Severity::Warning,
            Some(MemoryPressure::Normal | MemoryPressure::Unknown) | None => self.severity,
        }
    }

    fn relevant_logs<'a>(&self, _log_events: &'a [LogEvent]) -> Vec<&'a LogEvent> {
        Vec::new()
    }

    fn relevant_metrics<'a>(&self, metrics_events: &'a [MetricsEvent]) -> Vec<&'a MetricsEvent> {
        metrics_events
            .iter()
            .filter(|event| {
                event.provenance.memory_pressure == MeasurementKind::Measured
                    && event.memory_pressure >= self.threshold
            })
            .collect()
    }

    fn relevant_disk_events<'a>(&self, _disk_events: &'a [DiskEvent]) -> Vec<&'a DiskEvent> {
        Vec::new()
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
            crash_keywords: crash_keywords
                .into_iter()
                .map(|keyword| keyword.to_lowercase())
                .collect(),
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
        log_events.iter().any(|event| self.matches_event(event))
    }

    fn name(&self) -> &str {
        "CrashDetectionRule"
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn relevant_logs<'a>(&self, log_events: &'a [LogEvent]) -> Vec<&'a LogEvent> {
        log_events
            .iter()
            .filter(|event| self.matches_event(event))
            .collect()
    }

    fn relevant_log_groups<'a>(&self, log_events: &'a [LogEvent]) -> Vec<RelevantLogGroup<'a>> {
        let mut groups = BTreeMap::<ErrorSource, Vec<&LogEvent>>::new();
        for event in log_events.iter().filter(|event| self.matches_event(event)) {
            groups.entry(error_source(event)).or_default().push(event);
        }
        groups
            .into_iter()
            .map(|((process, subsystem, client), events)| RelevantLogGroup {
                source: Some(error_source_name(process, subsystem, client)),
                events,
            })
            .collect()
    }

    fn relevant_metrics<'a>(&self, _metrics_events: &'a [MetricsEvent]) -> Vec<&'a MetricsEvent> {
        Vec::new()
    }

    fn relevant_disk_events<'a>(&self, _disk_events: &'a [DiskEvent]) -> Vec<&'a DiskEvent> {
        Vec::new()
    }
}

impl CrashDetectionRule {
    fn matches_event(&self, event: &LogEvent) -> bool {
        if !matches!(event.message_type, MessageType::Error | MessageType::Fault)
            || event.is_known_benign_noise()
        {
            return false;
        }

        let message = event.message.to_lowercase();
        self.crash_keywords
            .iter()
            .any(|keyword| message.contains(keyword))
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

    fn relevant_logs<'a>(&self, _log_events: &'a [LogEvent]) -> Vec<&'a LogEvent> {
        Vec::new()
    }

    fn relevant_metrics<'a>(&self, metrics_events: &'a [MetricsEvent]) -> Vec<&'a MetricsEvent> {
        let cutoff = Utc::now() - Duration::seconds(self.comparison_window_seconds);
        metrics_events
            .iter()
            .filter(|event| event.timestamp >= cutoff)
            .collect()
    }

    fn relevant_disk_events<'a>(&self, _disk_events: &'a [DiskEvent]) -> Vec<&'a DiskEvent> {
        Vec::new()
    }
}

/// Trigger rule that activates when disk I/O activity spikes suddenly
///
/// This rule monitors disk read/write rates and triggers when I/O activity
/// increases significantly within a short time period, which could indicate
/// heavy disk usage, thrashing, or runaway processes.
#[derive(Debug)]
struct DiskSpike {
    disk_name: String,
    filesystem_path: Option<String>,
    operation: &'static str,
    baseline_kb_per_sec: f64,
    peak_kb_per_sec: f64,
    threshold_kb_per_sec: f64,
}

impl DiskSpike {
    fn delta_kb_per_sec(&self) -> f64 {
        self.peak_kb_per_sec - self.baseline_kb_per_sec
    }

    fn threshold_ratio(&self) -> f64 {
        self.delta_kb_per_sec() / self.threshold_kb_per_sec
    }

    fn evidence(&self) -> String {
        let source = self
            .filesystem_path
            .as_deref()
            .unwrap_or("mixed or unavailable");
        format!(
            "{} {} spike: baseline {:.1}KB/s, peak {:.1}KB/s, delta +{:.1}KB/s (threshold {:.1}KB/s), source {}",
            self.disk_name,
            self.operation,
            self.baseline_kb_per_sec,
            self.peak_kb_per_sec,
            self.delta_kb_per_sec(),
            self.threshold_kb_per_sec,
            source
        )
    }
}

pub struct DiskIOSpikeRule {
    /// Minimum disk read rate increase (in KB/s) to trigger
    pub read_spike_threshold_kb_per_sec: f64,
    /// Minimum disk write rate increase (in KB/s) to trigger
    pub write_spike_threshold_kb_per_sec: f64,
    /// Time window to compare current vs previous I/O (in seconds)
    pub comparison_window_seconds: i64,
    /// Severity level to assign when this rule triggers
    pub severity: Severity,
}

impl DiskIOSpikeRule {
    /// Create a new disk I/O spike rule
    ///
    /// # Arguments
    ///
    /// * `read_spike_threshold_kb_per_sec` - Minimum read rate increase (KB/s) to trigger
    /// * `write_spike_threshold_kb_per_sec` - Minimum write rate increase (KB/s) to trigger
    /// * `comparison_window_seconds` - Time window to compare current vs previous I/O
    /// * `severity` - Severity level to assign when this rule triggers
    pub fn new(
        read_spike_threshold_kb_per_sec: f64,
        write_spike_threshold_kb_per_sec: f64,
        comparison_window_seconds: i64,
        severity: Severity,
    ) -> Self {
        Self {
            read_spike_threshold_kb_per_sec,
            write_spike_threshold_kb_per_sec,
            comparison_window_seconds,
            severity,
        }
    }

    /// Create a default disk I/O spike rule (1MB/s read, 500KB/s write spike in 30 seconds)
    pub fn with_defaults() -> Self {
        Self::new(1024.0, 512.0, 30, Severity::Warning)
    }

    fn triggering_spikes(&self, disk_events: &[DiskEvent]) -> Vec<DiskSpike> {
        let cutoff = Utc::now() - Duration::seconds(self.comparison_window_seconds);
        let mut events_by_disk = BTreeMap::<String, Vec<&DiskEvent>>::new();

        for event in disk_events.iter().filter(|event| event.timestamp >= cutoff) {
            events_by_disk
                .entry(event.disk_name.clone())
                .or_default()
                .push(event);
        }

        let mut spikes = Vec::new();
        for (disk_name, mut events) in events_by_disk {
            if events.len() < 2 {
                continue;
            }
            events.sort_by_key(|event| event.timestamp);

            let mut read_min = events[0].read_kb_per_sec;
            let mut write_min = events[0].write_kb_per_sec;
            let mut largest_read = None::<(f64, f64, Option<String>)>;
            let mut largest_write = None::<(f64, f64, Option<String>)>;

            for event in &events[1..] {
                let read_delta = event.read_kb_per_sec - read_min;
                if largest_read
                    .as_ref()
                    .is_none_or(|(baseline, peak, _)| read_delta > peak - baseline)
                {
                    largest_read = Some((
                        read_min,
                        event.read_kb_per_sec,
                        event.filesystem_path.clone(),
                    ));
                }
                read_min = read_min.min(event.read_kb_per_sec);

                let write_delta = event.write_kb_per_sec - write_min;
                if largest_write
                    .as_ref()
                    .is_none_or(|(baseline, peak, _)| write_delta > peak - baseline)
                {
                    largest_write = Some((
                        write_min,
                        event.write_kb_per_sec,
                        event.filesystem_path.clone(),
                    ));
                }
                write_min = write_min.min(event.write_kb_per_sec);
            }

            if let Some((baseline, peak, filesystem_path)) = largest_read {
                if peak - baseline >= self.read_spike_threshold_kb_per_sec {
                    spikes.push(DiskSpike {
                        disk_name: disk_name.clone(),
                        filesystem_path,
                        operation: "read",
                        baseline_kb_per_sec: baseline,
                        peak_kb_per_sec: peak,
                        threshold_kb_per_sec: self.read_spike_threshold_kb_per_sec,
                    });
                }
            }
            if let Some((baseline, peak, filesystem_path)) = largest_write {
                if peak - baseline >= self.write_spike_threshold_kb_per_sec {
                    spikes.push(DiskSpike {
                        disk_name,
                        filesystem_path,
                        operation: "write",
                        baseline_kb_per_sec: baseline,
                        peak_kb_per_sec: peak,
                        threshold_kb_per_sec: self.write_spike_threshold_kb_per_sec,
                    });
                }
            }
        }

        spikes.sort_by(|left, right| right.threshold_ratio().total_cmp(&left.threshold_ratio()));
        spikes
    }
}

impl TriggerRule for DiskIOSpikeRule {
    fn evaluate(
        &self,
        _log_events: &[LogEvent],
        _metrics_events: &[MetricsEvent],
        disk_events: &[DiskEvent],
    ) -> bool {
        !self.triggering_spikes(disk_events).is_empty()
    }

    fn name(&self) -> &str {
        "DiskIOSpikeRule"
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn trigger_reason(
        &self,
        _log_events: &[LogEvent],
        _metrics_events: &[MetricsEvent],
        disk_events: &[DiskEvent],
        _source: Option<&str>,
    ) -> String {
        let evidence = self
            .triggering_spikes(disk_events)
            .into_iter()
            .map(|spike| spike.evidence())
            .collect::<Vec<_>>()
            .join("; ");
        format!("Rule '{}' triggered: {}", self.name(), evidence)
    }

    fn relevant_logs<'a>(&self, _log_events: &'a [LogEvent]) -> Vec<&'a LogEvent> {
        Vec::new()
    }

    fn relevant_metrics<'a>(&self, _metrics_events: &'a [MetricsEvent]) -> Vec<&'a MetricsEvent> {
        Vec::new()
    }

    fn relevant_disk_events<'a>(&self, disk_events: &'a [DiskEvent]) -> Vec<&'a DiskEvent> {
        let cutoff = Utc::now() - Duration::seconds(self.comparison_window_seconds);
        let triggering_disks = self
            .triggering_spikes(disk_events)
            .into_iter()
            .map(|spike| spike.disk_name)
            .collect::<HashSet<_>>();
        disk_events
            .iter()
            .filter(|event| {
                event.timestamp >= cutoff && triggering_disks.contains(&event.disk_name)
            })
            .collect()
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
                MemoryPressure::Unknown => 0.0,
                MemoryPressure::Normal => 2048.0,
                MemoryPressure::Warning => 6144.0,
                MemoryPressure::Critical => 12288.0,
            },
            energy_impact: cpu_power_mw + gpu_power_mw.unwrap_or(0.0),
            provenance: crate::events::MetricsProvenance {
                memory_pressure: MeasurementKind::Measured,
                ..crate::events::MetricsProvenance::default()
            },
            process_metrics: Vec::new(),
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
    fn test_error_frequency_counts_distinct_signatures() {
        let rule = ErrorFrequencyRule::new(1, 60, Severity::Warning);
        let repeated = (0..2)
            .map(|_| create_test_log_event(MessageType::Error, "Repeated error", 61))
            .chain((0..2).map(|_| create_test_log_event(MessageType::Error, "Repeated error", 1)))
            .collect::<Vec<_>>();

        assert!(!rule.evaluate(&repeated, &[], &[]));

        let mut distinct = repeated;
        distinct.push(create_test_log_event(
            MessageType::Error,
            "Different error",
            1,
        ));
        assert!(rule.evaluate(&distinct, &[], &[]));
    }

    #[test]
    fn test_error_frequency_does_not_combine_unrelated_sources() {
        let rule = ErrorFrequencyRule::new(1, 60, Severity::Warning);
        let mut first = create_test_log_event(MessageType::Error, "First error", 1);
        first.process = "first-process".to_string();
        first.subsystem = "first-subsystem".to_string();
        let mut second = create_test_log_event(MessageType::Error, "Second error", 1);
        second.process = "second-process".to_string();
        second.subsystem = "second-subsystem".to_string();

        assert!(!rule.evaluate(&[first, second], &[], &[]));
    }

    #[test]
    fn test_error_frequency_splits_runningboardd_clients() {
        let rule = ErrorFrequencyRule::new(1, 60, Severity::Warning);
        let mut first = create_test_log_event(
            MessageType::Error,
            "client not entitled for com.example.first",
            1,
        );
        first.process = "runningboardd".to_string();
        first.subsystem = "com.apple.runningboard".to_string();
        let mut second = create_test_log_event(
            MessageType::Error,
            "kernel coalition failure for com.example.second",
            1,
        );
        second.process = "runningboardd".to_string();
        second.subsystem = "com.apple.runningboard".to_string();

        assert!(!rule.evaluate(&[first, second], &[], &[]));
    }

    #[test]
    fn test_error_frequency_groups_runningboardd_errors_for_one_client() {
        let rule = ErrorFrequencyRule::new(1, 60, Severity::Warning);
        let mut first = create_test_log_event(
            MessageType::Error,
            "client not entitled for com.example.client",
            1,
        );
        first.process = "runningboardd".to_string();
        first.subsystem = "com.apple.runningboard".to_string();
        let mut second = create_test_log_event(
            MessageType::Error,
            "kernel coalition failure for com.example.client",
            1,
        );
        second.process = "runningboardd".to_string();
        second.subsystem = "com.apple.runningboard".to_string();

        let events = [first, second];
        assert!(rule.evaluate(&events, &[], &[]));
        let groups = rule.relevant_log_groups(&events);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].events.len(), 2);
        assert_eq!(
            groups[0].source.as_deref(),
            Some("com.apple.runningboard/runningboardd client com.example.client")
        );
    }

    #[test]
    fn test_error_frequency_triggers_on_repeated_signature_rate_spike() {
        let rule = ErrorFrequencyRule::new(5, 60, Severity::Warning);
        let events = std::iter::once(create_test_log_event(
            MessageType::Error,
            "Repeated error",
            61,
        ))
        .chain((0..6).map(|_| create_test_log_event(MessageType::Error, "Repeated error", 1)))
        .collect::<Vec<_>>();

        assert!(rule.evaluate(&events, &[], &[]));
    }

    #[test]
    fn test_error_frequency_ignores_known_benign_noise() {
        let rule = ErrorFrequencyRule::new(0, 60, Severity::Warning);
        let mut event = create_test_log_event(
            MessageType::Error,
            "dispatch_mig_server returned 268435459",
            1,
        );
        event.process = "syspolicyd".to_string();
        event.process_id = 473;

        assert!(!rule.evaluate(&[event], &[], &[]));
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
    fn test_memory_pressure_rule_uses_observed_severity() {
        let rule = MemoryPressureRule::new(MemoryPressure::Warning, Severity::Warning);
        let critical = create_test_metrics_event(1000.0, None, MemoryPressure::Critical, 1);

        assert_eq!(rule.severity_for(&[], &[critical], &[]), Severity::Critical);
    }

    #[test]
    fn test_memory_pressure_rule_ignores_unavailable_pressure() {
        let rule = MemoryPressureRule::new(MemoryPressure::Warning, Severity::Warning);
        let unknown = create_test_metrics_event(1000.0, None, MemoryPressure::Unknown, 1);

        assert!(!rule.evaluate(&[], &[unknown], &[]));
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
    fn test_crash_detection_rule_ignores_context_store_simulation() {
        let rule = CrashDetectionRule::with_defaults();
        let mut event =
            create_test_log_event(MessageType::Error, "Simulating crash. Reason: <private>", 1);
        event.process = "ContextStoreAgent".to_string();

        assert!(!rule.evaluate(&[event], &[], &[]));
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
    fn test_crash_detection_groups_evidence_by_process() {
        let rule = CrashDetectionRule::with_defaults();
        let mut editor_crash = create_test_log_event(MessageType::Error, "Application crashed", 2);
        editor_crash.process = "ExampleEditor".to_string();
        editor_crash.subsystem = "com.example.editor".to_string();
        let mut browser_crash = create_test_log_event(MessageType::Fault, "Segmentation fault", 1);
        browser_crash.process = "ExampleBrowser".to_string();
        browser_crash.subsystem = "com.example.browser".to_string();

        let events = vec![editor_crash, browser_crash];
        let groups = rule.relevant_log_groups(&events);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().any(|group| {
            group.source.as_deref() == Some("com.example.editor/ExampleEditor")
                && group.events[0].process == "ExampleEditor"
        }));
        assert!(groups.iter().any(|group| {
            group.source.as_deref() == Some("com.example.browser/ExampleBrowser")
                && group.events[0].process == "ExampleBrowser"
        }));
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

        let disk_rule = DiskIOSpikeRule::with_defaults();
        assert_eq!(disk_rule.name(), "DiskIOSpikeRule");
        assert_eq!(disk_rule.severity(), Severity::Warning);
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
    fn test_disk_io_spike_rule_no_trigger_insufficient_data() {
        let rule = DiskIOSpikeRule::new(1024.0, 512.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![];
        let disk_events = vec![create_test_disk_event(100.0, 50.0, "disk0", 10)];

        // Should not trigger with only 1 data point
        assert!(!rule.evaluate(&log_events, &metrics_events, &disk_events));
    }

    #[test]
    fn test_disk_io_spike_rule_no_trigger_small_increase() {
        let rule = DiskIOSpikeRule::new(1024.0, 512.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![];
        let disk_events = vec![
            create_test_disk_event(100.0, 50.0, "disk0", 25), // Earlier
            create_test_disk_event(200.0, 100.0, "disk0", 10), // Later
        ];

        // Read increase: 200 - 100 = 100KB/s (below 1024KB/s threshold)
        // Write increase: 100 - 50 = 50KB/s (below 512KB/s threshold)
        assert!(!rule.evaluate(&log_events, &metrics_events, &disk_events));
    }

    #[test]
    fn test_disk_io_spike_rule_trigger_read_spike() {
        let rule = DiskIOSpikeRule::new(1024.0, 512.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![];
        let disk_events = vec![
            create_test_disk_event(100.0, 50.0, "disk0", 25), // Earlier
            create_test_disk_event(2000.0, 100.0, "disk0", 10), // Later
        ];

        // Read increase: 2000 - 100 = 1900KB/s (above 1024KB/s threshold)
        assert!(rule.evaluate(&log_events, &metrics_events, &disk_events));
    }

    #[test]
    fn test_disk_io_spike_rule_trigger_write_spike() {
        let rule = DiskIOSpikeRule::new(1024.0, 512.0, 30, Severity::Warning);

        let log_events = vec![];
        let metrics_events = vec![];
        let disk_events = vec![
            create_test_disk_event(100.0, 50.0, "disk0", 25), // Earlier
            create_test_disk_event(200.0, 1000.0, "disk0", 10), // Later
        ];

        // Read increase: 200 - 100 = 100KB/s (below 1024KB/s threshold)
        // Write increase: 1000 - 50 = 950KB/s (above 512KB/s threshold)
        assert!(rule.evaluate(&log_events, &metrics_events, &disk_events));
    }

    #[test]
    fn test_disk_io_spike_rule_time_window() {
        let rule = DiskIOSpikeRule::new(1024.0, 512.0, 20, Severity::Warning); // 20 second window

        let log_events = vec![];
        let metrics_events = vec![];
        let disk_events = vec![
            create_test_disk_event(100.0, 50.0, "disk0", 30), // Outside window
            create_test_disk_event(200.0, 100.0, "disk0", 15), // In window
            create_test_disk_event(2000.0, 150.0, "disk0", 5), // In window
        ];

        // Should compare events within 20s window: 200 -> 2000 = 1800KB/s increase (above threshold)
        assert!(rule.evaluate(&log_events, &metrics_events, &disk_events));
    }

    #[test]
    fn test_disk_io_spike_rule_does_not_compare_different_devices() {
        let rule = DiskIOSpikeRule::new(1024.0, 512.0, 30, Severity::Warning);
        let disk_events = vec![
            create_test_disk_event(100.0, 50.0, "disk0", 25),
            create_test_disk_event(2000.0, 1000.0, "disk1", 10),
        ];

        assert!(!rule.evaluate(&[], &[], &disk_events));
    }

    #[test]
    fn test_disk_io_spike_rule_reports_trigger_measurements_and_source() {
        let rule = DiskIOSpikeRule::new(1024.0, 512.0, 30, Severity::Warning);
        let mut baseline = create_test_disk_event(100.0, 50.0, "fs_usage", 25);
        baseline.filesystem_path = None;
        let mut peak = create_test_disk_event(1800.0, 100.0, "fs_usage", 10);
        peak.filesystem_path = Some("/Users/example/project".to_string());
        let disk_events = vec![baseline, peak];

        let reason = rule.trigger_reason(&[], &[], &disk_events, None);

        assert!(reason.contains("fs_usage read spike"));
        assert!(reason.contains("baseline 100.0KB/s"));
        assert!(reason.contains("peak 1800.0KB/s"));
        assert!(reason.contains("delta +1700.0KB/s"));
        assert!(reason.contains("threshold 1024.0KB/s"));
        assert!(reason.contains("source /Users/example/project"));
    }

    fn create_test_disk_event(
        read_kb_per_sec: f64,
        write_kb_per_sec: f64,
        disk_name: &str,
        timestamp_offset_seconds: i64,
    ) -> DiskEvent {
        DiskEvent {
            timestamp: Utc::now() - Duration::seconds(timestamp_offset_seconds),
            read_kb_per_sec,
            write_kb_per_sec,
            read_ops_per_sec: read_kb_per_sec / 4.0, // Approximate ops from KB
            write_ops_per_sec: write_kb_per_sec / 4.0,
            disk_name: disk_name.to_string(),
            filesystem_path: Some("/".to_string()),
        }
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

        let disk_rule = DiskIOSpikeRule::with_defaults();
        assert_eq!(disk_rule.read_spike_threshold_kb_per_sec, 1024.0);
        assert_eq!(disk_rule.write_spike_threshold_kb_per_sec, 512.0);
        assert_eq!(disk_rule.comparison_window_seconds, 30);
        assert_eq!(disk_rule.severity(), Severity::Warning);
    }
}
