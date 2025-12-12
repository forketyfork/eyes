//! Self-monitoring metrics collection for the SystemObserver
//!
//! This module provides functionality to track the performance and health
//! of the SystemObserver application itself, including memory usage,
//! event processing rates, AI analysis latency, and notification delivery success rates.

use chrono::{DateTime, Utc};
use log::{debug, info, warn};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Self-monitoring metrics for the SystemObserver application
#[derive(Debug, Clone)]
pub struct SelfMonitoringMetrics {
    /// Current memory usage in bytes
    pub memory_usage_bytes: u64,
    /// Number of log events processed in the last minute
    pub log_events_per_minute: u64,
    /// Number of metrics events processed in the last minute
    pub metrics_events_per_minute: u64,
    /// Average AI analysis latency in milliseconds
    pub avg_ai_analysis_latency_ms: f64,
    /// Number of successful notifications in the last minute
    pub successful_notifications_per_minute: u64,
    /// Number of failed notifications in the last minute
    pub failed_notifications_per_minute: u64,
    /// Notification success rate as a percentage (0-100)
    pub notification_success_rate: f64,
    /// Timestamp when these metrics were collected
    pub timestamp: DateTime<Utc>,
}

/// Tracks timing information for AI analysis operations
#[derive(Debug, Clone)]
struct AnalysisLatency {
    duration: Duration,
    #[allow(dead_code)] // Reserved for future time-based analysis features
    timestamp: DateTime<Utc>,
}

/// Tracks notification delivery results
#[derive(Debug, Clone)]
struct NotificationResult {
    success: bool,
    timestamp: DateTime<Utc>,
}

/// Event processing counter
#[derive(Debug, Clone)]
struct EventCount {
    count: u64,
    timestamp: DateTime<Utc>,
}

/// Self-monitoring collector that tracks application performance metrics
#[derive(Debug)]
pub struct SelfMonitoringCollector {
    /// Recent AI analysis latencies (last 100 operations)
    analysis_latencies: Arc<Mutex<VecDeque<AnalysisLatency>>>,
    /// Recent notification results (last 1000 notifications)
    notification_results: Arc<Mutex<VecDeque<NotificationResult>>>,
    /// Recent log event counts (per minute buckets)
    log_event_counts: Arc<Mutex<VecDeque<EventCount>>>,
    /// Recent metrics event counts (per minute buckets)
    metrics_event_counts: Arc<Mutex<VecDeque<EventCount>>>,
    /// Maximum number of latency samples to keep
    max_latency_samples: usize,
    /// Maximum number of notification results to keep
    max_notification_samples: usize,
    /// Maximum age for event count buckets
    max_count_age: Duration,
}

impl Default for SelfMonitoringCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl SelfMonitoringCollector {
    /// Create a new self-monitoring collector
    pub fn new() -> Self {
        Self {
            analysis_latencies: Arc::new(Mutex::new(VecDeque::new())),
            notification_results: Arc::new(Mutex::new(VecDeque::new())),
            log_event_counts: Arc::new(Mutex::new(VecDeque::new())),
            metrics_event_counts: Arc::new(Mutex::new(VecDeque::new())),
            max_latency_samples: 100,
            max_notification_samples: 1000,
            max_count_age: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Record the latency of an AI analysis operation
    pub fn record_ai_analysis_latency(&self, duration: Duration) {
        debug!("Recording AI analysis latency: {:?}", duration);

        let latency = AnalysisLatency {
            duration,
            timestamp: Utc::now(),
        };

        let mut latencies = self.analysis_latencies.lock().unwrap();
        latencies.push_back(latency);

        // Keep only the most recent samples
        while latencies.len() > self.max_latency_samples {
            latencies.pop_front();
        }

        debug!(
            "AI analysis latency recorded, total samples: {}",
            latencies.len()
        );
    }

    /// Record the result of a notification delivery attempt
    pub fn record_notification_result(&self, success: bool) {
        debug!("Recording notification result: success={}", success);

        let result = NotificationResult {
            success,
            timestamp: Utc::now(),
        };

        let mut results = self.notification_results.lock().unwrap();
        results.push_back(result);

        // Keep only the most recent samples
        while results.len() > self.max_notification_samples {
            results.pop_front();
        }

        debug!(
            "Notification result recorded, total samples: {}",
            results.len()
        );
    }

    /// Record that log events were processed
    pub fn record_log_events_processed(&self, count: u64) {
        if count == 0 {
            return;
        }

        debug!("Recording {} log events processed", count);

        let event_count = EventCount {
            count,
            timestamp: Utc::now(),
        };

        let mut counts = self.log_event_counts.lock().unwrap();
        counts.push_back(event_count);

        // Remove old entries
        let cutoff = Utc::now() - chrono::Duration::from_std(self.max_count_age).unwrap();
        while let Some(front) = counts.front() {
            if front.timestamp < cutoff {
                counts.pop_front();
            } else {
                break;
            }
        }

        debug!("Log event count recorded, total buckets: {}", counts.len());
    }

    /// Record that metrics events were processed
    pub fn record_metrics_events_processed(&self, count: u64) {
        if count == 0 {
            return;
        }

        debug!("Recording {} metrics events processed", count);

        let event_count = EventCount {
            count,
            timestamp: Utc::now(),
        };

        let mut counts = self.metrics_event_counts.lock().unwrap();
        counts.push_back(event_count);

        // Remove old entries
        let cutoff = Utc::now() - chrono::Duration::from_std(self.max_count_age).unwrap();
        while let Some(front) = counts.front() {
            if front.timestamp < cutoff {
                counts.pop_front();
            } else {
                break;
            }
        }

        debug!(
            "Metrics event count recorded, total buckets: {}",
            counts.len()
        );
    }

    /// Get current memory usage of the application
    fn get_memory_usage(&self) -> u64 {
        #[cfg(target_os = "macos")]
        {
            // Use macOS-specific approach to get current resident memory size
            // We'll use a simpler approach that works with current libc
            use std::process::Command;

            // Use ps command to get RSS for current process
            if let Ok(output) = Command::new("ps")
                .args(["-o", "rss=", "-p", &std::process::id().to_string()])
                .output()
            {
                if let Ok(output_str) = String::from_utf8(output.stdout) {
                    if let Ok(rss_kb) = output_str.trim().parse::<u64>() {
                        return rss_kb * 1024; // Convert KB to bytes
                    }
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            use std::fs;

            // Try to read memory usage from /proc/self/status
            if let Ok(status) = fs::read_to_string("/proc/self/status") {
                for line in status.lines() {
                    if line.starts_with("VmRSS:") {
                        if let Some(kb_str) = line.split_whitespace().nth(1) {
                            if let Ok(kb) = kb_str.parse::<u64>() {
                                return kb * 1024; // Convert KB to bytes
                            }
                        }
                    }
                }
            }
        }

        #[cfg(unix)]
        {
            // Fallback: try to use rusage (but this gives peak usage, not current)
            unsafe {
                let mut usage = std::mem::zeroed();
                if libc::getrusage(libc::RUSAGE_SELF, &mut usage) == 0 {
                    // ru_maxrss is in KB on Linux, bytes on macOS
                    #[cfg(target_os = "linux")]
                    return (usage.ru_maxrss * 1024) as u64;

                    #[cfg(target_os = "macos")]
                    return usage.ru_maxrss as u64;
                }
            }
        }

        // Fallback: return 0 if we can't determine memory usage
        0
    }

    /// Calculate average AI analysis latency from recent samples
    fn calculate_avg_ai_latency(&self) -> f64 {
        let latencies = self.analysis_latencies.lock().unwrap();

        if latencies.is_empty() {
            return 0.0;
        }

        let total_ms: f64 = latencies
            .iter()
            .map(|l| l.duration.as_millis() as f64)
            .sum();

        total_ms / latencies.len() as f64
    }

    /// Calculate event processing rates per minute
    fn calculate_event_rates(&self) -> (u64, u64) {
        let now = Utc::now();
        let one_minute_ago = now - chrono::Duration::minutes(1);

        // Calculate log events per minute
        let log_counts = self.log_event_counts.lock().unwrap();
        let log_events_per_minute: u64 = log_counts
            .iter()
            .filter(|count| count.timestamp >= one_minute_ago)
            .map(|count| count.count)
            .sum();

        // Calculate metrics events per minute
        let metrics_counts = self.metrics_event_counts.lock().unwrap();
        let metrics_events_per_minute: u64 = metrics_counts
            .iter()
            .filter(|count| count.timestamp >= one_minute_ago)
            .map(|count| count.count)
            .sum();

        (log_events_per_minute, metrics_events_per_minute)
    }

    /// Calculate notification success rate from recent samples
    fn calculate_notification_success_rate(&self) -> (u64, u64, f64) {
        let now = Utc::now();
        let one_minute_ago = now - chrono::Duration::minutes(1);

        let results = self.notification_results.lock().unwrap();

        let recent_results: Vec<_> = results
            .iter()
            .filter(|result| result.timestamp >= one_minute_ago)
            .collect();

        if recent_results.is_empty() {
            return (0, 0, 100.0); // No recent notifications, assume 100% success rate
        }

        let successful = recent_results.iter().filter(|r| r.success).count() as u64;
        let failed = recent_results.len() as u64 - successful;
        let success_rate = (successful as f64 / recent_results.len() as f64) * 100.0;

        (successful, failed, success_rate)
    }

    /// Collect current self-monitoring metrics
    pub fn collect_metrics(&self) -> SelfMonitoringMetrics {
        debug!("Collecting self-monitoring metrics");

        let memory_usage_bytes = self.get_memory_usage();
        let avg_ai_analysis_latency_ms = self.calculate_avg_ai_latency();
        let (log_events_per_minute, metrics_events_per_minute) = self.calculate_event_rates();
        let (
            successful_notifications_per_minute,
            failed_notifications_per_minute,
            notification_success_rate,
        ) = self.calculate_notification_success_rate();

        let metrics = SelfMonitoringMetrics {
            memory_usage_bytes,
            log_events_per_minute,
            metrics_events_per_minute,
            avg_ai_analysis_latency_ms,
            successful_notifications_per_minute,
            failed_notifications_per_minute,
            notification_success_rate,
            timestamp: Utc::now(),
        };

        info!("Self-monitoring metrics: memory={}MB, log_events/min={}, metrics_events/min={}, ai_latency={:.1}ms, notification_success={:.1}%",
              memory_usage_bytes / 1024 / 1024,
              log_events_per_minute,
              metrics_events_per_minute,
              avg_ai_analysis_latency_ms,
              notification_success_rate);

        // Warn about potential issues
        if memory_usage_bytes > 500 * 1024 * 1024 {
            // 500MB
            warn!(
                "High memory usage detected: {}MB",
                memory_usage_bytes / 1024 / 1024
            );
        }

        if avg_ai_analysis_latency_ms > 30000.0 {
            // 30 seconds
            warn!(
                "High AI analysis latency detected: {:.1}ms",
                avg_ai_analysis_latency_ms
            );
        }

        if notification_success_rate < 90.0
            && (successful_notifications_per_minute + failed_notifications_per_minute) > 0
        {
            warn!(
                "Low notification success rate: {:.1}%",
                notification_success_rate
            );
        }

        metrics
    }

    /// Get a clone of the collector for sharing across threads
    pub fn clone_collector(&self) -> Self {
        Self {
            analysis_latencies: Arc::clone(&self.analysis_latencies),
            notification_results: Arc::clone(&self.notification_results),
            log_event_counts: Arc::clone(&self.log_event_counts),
            metrics_event_counts: Arc::clone(&self.metrics_event_counts),
            max_latency_samples: self.max_latency_samples,
            max_notification_samples: self.max_notification_samples,
            max_count_age: self.max_count_age,
        }
    }
}

/// Helper struct to measure and automatically record AI analysis latency
pub struct AnalysisTimer {
    start_time: Instant,
    collector: Arc<SelfMonitoringCollector>,
}

impl AnalysisTimer {
    /// Start timing an AI analysis operation
    pub fn start(collector: Arc<SelfMonitoringCollector>) -> Self {
        Self {
            start_time: Instant::now(),
            collector,
        }
    }

    /// Finish timing and record the latency
    pub fn finish(self) {
        let duration = self.start_time.elapsed();
        self.collector.record_ai_analysis_latency(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration as StdDuration;

    #[test]
    fn test_self_monitoring_collector_creation() {
        let collector = SelfMonitoringCollector::new();
        let metrics = collector.collect_metrics();

        // Initial metrics should be mostly zero
        assert_eq!(metrics.log_events_per_minute, 0);
        assert_eq!(metrics.metrics_events_per_minute, 0);
        assert_eq!(metrics.avg_ai_analysis_latency_ms, 0.0);
        assert_eq!(metrics.successful_notifications_per_minute, 0);
        assert_eq!(metrics.failed_notifications_per_minute, 0);
        assert_eq!(metrics.notification_success_rate, 100.0); // No notifications = 100% success
    }

    #[test]
    fn test_ai_analysis_latency_recording() {
        let collector = SelfMonitoringCollector::new();

        // Record some latencies
        collector.record_ai_analysis_latency(StdDuration::from_millis(100));
        collector.record_ai_analysis_latency(StdDuration::from_millis(200));
        collector.record_ai_analysis_latency(StdDuration::from_millis(300));

        let metrics = collector.collect_metrics();

        // Average should be 200ms
        assert!((metrics.avg_ai_analysis_latency_ms - 200.0).abs() < 1.0);
    }

    #[test]
    fn test_notification_result_recording() {
        let collector = SelfMonitoringCollector::new();

        // Record some notification results
        collector.record_notification_result(true);
        collector.record_notification_result(true);
        collector.record_notification_result(false);
        collector.record_notification_result(true);

        let metrics = collector.collect_metrics();

        // Should have 3 successful, 1 failed, 75% success rate
        assert_eq!(metrics.successful_notifications_per_minute, 3);
        assert_eq!(metrics.failed_notifications_per_minute, 1);
        assert!((metrics.notification_success_rate - 75.0).abs() < 1.0);
    }

    #[test]
    fn test_event_count_recording() {
        let collector = SelfMonitoringCollector::new();

        // Record some event processing
        collector.record_log_events_processed(10);
        collector.record_log_events_processed(5);
        collector.record_metrics_events_processed(3);
        collector.record_metrics_events_processed(7);

        let metrics = collector.collect_metrics();

        // Should sum up the events
        assert_eq!(metrics.log_events_per_minute, 15);
        assert_eq!(metrics.metrics_events_per_minute, 10);
    }

    #[test]
    fn test_analysis_timer() {
        let collector = Arc::new(SelfMonitoringCollector::new());

        // Start a timer
        let timer = AnalysisTimer::start(Arc::clone(&collector));

        // Simulate some work
        thread::sleep(StdDuration::from_millis(10));

        // Finish the timer
        timer.finish();

        let metrics = collector.collect_metrics();

        // Should have recorded some latency
        assert!(metrics.avg_ai_analysis_latency_ms > 0.0);
        assert!(metrics.avg_ai_analysis_latency_ms >= 10.0); // At least 10ms
    }

    #[test]
    fn test_collector_cloning() {
        let collector1 = SelfMonitoringCollector::new();
        let collector2 = collector1.clone_collector();

        // Record data in collector1
        collector1.record_ai_analysis_latency(StdDuration::from_millis(100));
        collector1.record_notification_result(true);

        // collector2 should see the same data (shared Arc)
        let metrics1 = collector1.collect_metrics();
        let metrics2 = collector2.collect_metrics();

        assert_eq!(
            metrics1.avg_ai_analysis_latency_ms,
            metrics2.avg_ai_analysis_latency_ms
        );
        assert_eq!(
            metrics1.successful_notifications_per_minute,
            metrics2.successful_notifications_per_minute
        );
    }

    #[test]
    fn test_memory_usage_collection() {
        let collector = SelfMonitoringCollector::new();
        let metrics = collector.collect_metrics();

        // Memory usage should be a valid value (might be 0 if we can't determine it)
        // Since u64 is always >= 0, we just check that the call doesn't panic
        let _ = metrics.memory_usage_bytes;
    }

    #[test]
    fn test_latency_sample_limit() {
        let collector = SelfMonitoringCollector::new();

        // Record more samples than the limit
        for i in 0..150 {
            collector.record_ai_analysis_latency(StdDuration::from_millis(i));
        }

        // Should only keep the most recent samples
        let latencies = collector.analysis_latencies.lock().unwrap();
        assert_eq!(latencies.len(), collector.max_latency_samples);

        // The oldest samples should be the most recent ones we added
        let first_latency = latencies.front().unwrap();
        assert!(first_latency.duration.as_millis() >= 50); // Should be from the later samples
    }

    #[test]
    fn test_notification_sample_limit() {
        let collector = SelfMonitoringCollector::new();

        // Record more samples than the limit
        for i in 0..1200 {
            collector.record_notification_result(i % 2 == 0);
        }

        // Should only keep the most recent samples
        let results = collector.notification_results.lock().unwrap();
        assert_eq!(results.len(), collector.max_notification_samples);
    }
}
