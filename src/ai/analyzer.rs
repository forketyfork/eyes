use crate::ai::backends::LLMBackend;
use crate::error::AnalysisError;
use crate::events::{LogEvent, MetricsEvent, Severity, Timestamp};
use crate::monitoring::{AnalysisTimer, SelfMonitoringCollector};
use crate::triggers::TriggerContext;
use chrono::Utc;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Retry queue entry for failed AI analysis requests
#[derive(Debug, Clone)]
struct RetryEntry {
    context: TriggerContext,
    attempt_count: u32,
    next_retry_time: Instant,
}

/// AI-powered system analysis coordinator
///
/// The AIAnalyzer receives trigger contexts containing recent system events
/// and coordinates with LLM backends to generate actionable insights.
/// It includes a retry queue for handling backend failures gracefully.
pub struct AIAnalyzer {
    backend: Arc<dyn LLMBackend>,
    monitoring: Option<Arc<SelfMonitoringCollector>>,
    retry_queue: Arc<Mutex<VecDeque<RetryEntry>>>,
    max_retry_attempts: u32,
    max_queue_size: usize,
    base_retry_delay: Duration,
}

/// AI-generated insight about system behavior
///
/// Represents the result of AI analysis, including severity assessment,
/// root cause analysis, and recommended actions. This structure matches
/// the expected JSON response format from LLM backends.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AIInsight {
    /// When this insight was generated
    pub timestamp: Timestamp,
    /// Brief description of the main issue or finding
    pub summary: String,
    /// Most likely underlying cause of the issue (optional)
    pub root_cause: Option<String>,
    /// Specific actionable steps the user can take
    pub recommendations: Vec<String>,
    /// Severity level: "info", "warning", or "critical"
    pub severity: Severity,
}

impl Default for AIAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl AIAnalyzer {
    /// Create a new AI analyzer with a placeholder backend
    ///
    /// Note: This creates a non-functional analyzer for testing.
    /// Use `with_backend` for production usage.
    pub fn new() -> Self {
        Self {
            backend: Arc::new(PlaceholderBackend),
            monitoring: None,
            retry_queue: Arc::new(Mutex::new(VecDeque::new())),
            max_retry_attempts: 3,
            max_queue_size: 100,
            base_retry_delay: Duration::from_secs(1),
        }
    }

    /// Create an AI analyzer with a specific LLM backend
    pub fn with_backend(backend: Arc<dyn LLMBackend>) -> Self {
        Self {
            backend,
            monitoring: None,
            retry_queue: Arc::new(Mutex::new(VecDeque::new())),
            max_retry_attempts: 3,
            max_queue_size: 100,
            base_retry_delay: Duration::from_secs(1),
        }
    }

    /// Set the self-monitoring collector for tracking analysis latency
    pub fn set_monitoring(&mut self, monitoring: Arc<SelfMonitoringCollector>) {
        self.monitoring = Some(monitoring);
    }

    /// Add a failed analysis to the retry queue
    fn queue_for_retry(&self, context: TriggerContext) {
        let mut queue = self.retry_queue.lock().unwrap();

        // Check if queue is full
        if queue.len() >= self.max_queue_size {
            warn!("Retry queue is full, dropping oldest entry");
            queue.pop_front();
        }

        let retry_entry = RetryEntry {
            context,
            attempt_count: 1,
            next_retry_time: Instant::now() + self.base_retry_delay,
        };

        queue.push_back(retry_entry);
        info!("Queued analysis for retry, queue size: {}", queue.len());
    }

    /// Process any pending retries that are ready
    pub async fn process_retry_queue(&self) -> Vec<Result<AIInsight, AnalysisError>> {
        let mut results = Vec::new();
        let now = Instant::now();

        // Get entries ready for retry
        let mut ready_entries = Vec::new();
        {
            let mut queue = self.retry_queue.lock().unwrap();
            let mut i = 0;
            while i < queue.len() {
                if queue[i].next_retry_time <= now {
                    ready_entries.push(queue.remove(i).unwrap());
                } else {
                    i += 1;
                }
            }
        }

        // Process ready entries
        for mut entry in ready_entries {
            debug!(
                "Retrying analysis attempt {} for trigger: {}",
                entry.attempt_count + 1,
                entry.context.triggered_by
            );

            match self.analyze_without_retry(&entry.context).await {
                Ok(insight) => {
                    info!(
                        "Retry successful for trigger: {}",
                        entry.context.triggered_by
                    );
                    results.push(Ok(insight));
                }
                Err(e) => {
                    entry.attempt_count += 1;

                    if entry.attempt_count < self.max_retry_attempts {
                        // Calculate exponential backoff delay
                        let delay = self.base_retry_delay * 2_u32.pow(entry.attempt_count - 1);
                        entry.next_retry_time = now + delay;

                        // Re-queue for another retry
                        let mut queue = self.retry_queue.lock().unwrap();
                        if queue.len() < self.max_queue_size {
                            queue.push_back(entry);
                            debug!("Re-queued for retry with delay: {:?}", delay);
                        } else {
                            warn!("Retry queue full, dropping failed retry");
                            results.push(Err(e));
                        }
                    } else {
                        error!(
                            "Max retry attempts reached for trigger: {}",
                            entry.context.triggered_by
                        );
                        results.push(Err(e));
                    }
                }
            }
        }

        results
    }

    /// Get the current retry queue size
    pub fn retry_queue_size(&self) -> usize {
        self.retry_queue.lock().unwrap().len()
    }

    /// Analyze a trigger context and generate insights
    ///
    /// This is the main entry point for AI analysis. It takes a trigger context
    /// containing recent system events and returns actionable insights.
    /// If the analysis fails, the request is queued for retry.
    ///
    /// # Errors
    ///
    /// Returns `AnalysisError` if:
    /// - The backend communication fails
    /// - The response format is invalid
    /// - A timeout occurs during analysis
    pub async fn analyze(&self, context: &TriggerContext) -> Result<AIInsight, AnalysisError> {
        match self.analyze_without_retry(context).await {
            Ok(insight) => Ok(insight),
            Err(e) => {
                // Queue for retry as per Requirement 7.3
                warn!("AI analysis failed, queuing for retry: {}", e);
                self.queue_for_retry(context.clone());
                Err(e)
            }
        }
    }

    /// Analyze a trigger context without retry queue handling
    ///
    /// This method performs the actual analysis without adding failed requests
    /// to the retry queue. Used internally by both analyze() and retry processing.
    async fn analyze_without_retry(
        &self,
        context: &TriggerContext,
    ) -> Result<AIInsight, AnalysisError> {
        info!(
            "Starting AI analysis for trigger: '{}' with {} log events and {} metrics events",
            context.triggered_by,
            context.log_events.len(),
            context.metrics_events.len()
        );

        let summary = context.event_summary();
        debug!(
            "Event summary: errors={}, faults={}, total_logs={}, total_metrics={}",
            summary.error_count,
            summary.fault_count,
            summary.total_log_events,
            summary.total_metrics_events
        );

        let start_time = std::time::Instant::now();

        // Start timing the backend call if monitoring is available
        let timer = self
            .monitoring
            .as_ref()
            .map(|m| AnalysisTimer::start(m.clone()));

        // Delegate to the backend for actual analysis
        let result = self.backend.analyze(context).await;

        // Finish timing the backend call
        if let Some(timer) = timer {
            timer.finish();
        }

        let duration = start_time.elapsed();

        match &result {
            Ok(insight) => {
                info!(
                    "AI analysis completed successfully in {:?}: severity={:?}, summary='{}'",
                    duration, insight.severity, insight.summary
                );
                debug!(
                    "AI analysis details: root_cause={:?}, recommendations_count={}",
                    insight.root_cause,
                    insight.recommendations.len()
                );
            }
            Err(e) => {
                error!("AI analysis failed after {:?}: {}", duration, e);
            }
        }

        result
    }

    /// Generate a summary of recent system activity
    ///
    /// This method provides a high-level overview of system behavior
    /// without requiring a specific trigger condition.
    pub async fn summarize_activity(
        &self,
        log_events: &[LogEvent],
        metrics_events: &[MetricsEvent],
    ) -> Result<AIInsight, AnalysisError> {
        // Create a synthetic trigger context for summary analysis
        let context = TriggerContext::for_summary(log_events, metrics_events);
        self.analyze(&context).await
    }

    /// Format a trigger context into a structured prompt for LLM analysis
    ///
    /// This method creates a comprehensive prompt that includes:
    /// - System context (time window, event counts, memory pressure)
    /// - Recent error logs with timestamps and details
    /// - Resource metrics including CPU, GPU, and energy consumption
    /// - Clear instructions for the expected response format
    pub fn format_prompt(&self, context: &TriggerContext) -> String {
        let summary = context.event_summary();
        let time_range = context.time_range();

        // Calculate time window duration
        let duration = if let Some((start, end)) = time_range {
            let duration = end.signed_duration_since(start);
            format!("{} seconds", duration.num_seconds())
        } else {
            "unknown".to_string()
        };

        // Extract memory pressure information from metrics
        let memory_pressure = if !context.metrics_events.is_empty() {
            let latest_metrics = &context.metrics_events[context.metrics_events.len() - 1];
            format!("{:?}", latest_metrics.memory_pressure)
        } else {
            "Unknown".to_string()
        };

        // Calculate average CPU and GPU usage, memory usage, and energy impact
        let (
            avg_cpu_usage,
            avg_cpu_power,
            avg_gpu_usage,
            avg_gpu_power,
            avg_memory_used,
            avg_energy_impact,
        ) = if !context.metrics_events.is_empty() {
            let cpu_usage_sum: f64 = context
                .metrics_events
                .iter()
                .map(|m| m.cpu_usage_percent)
                .sum();
            let avg_cpu_usage = cpu_usage_sum / context.metrics_events.len() as f64;

            let cpu_power_sum: f64 = context.metrics_events.iter().map(|m| m.cpu_power_mw).sum();
            let avg_cpu_power = cpu_power_sum / context.metrics_events.len() as f64;

            let gpu_usage_values: Vec<f64> = context
                .metrics_events
                .iter()
                .filter_map(|m| m.gpu_usage_percent)
                .collect();
            let avg_gpu_usage = if !gpu_usage_values.is_empty() {
                Some(gpu_usage_values.iter().sum::<f64>() / gpu_usage_values.len() as f64)
            } else {
                None
            };

            let gpu_power_values: Vec<f64> = context
                .metrics_events
                .iter()
                .filter_map(|m| m.gpu_power_mw)
                .collect();
            let avg_gpu_power = if !gpu_power_values.is_empty() {
                Some(gpu_power_values.iter().sum::<f64>() / gpu_power_values.len() as f64)
            } else {
                None
            };

            let memory_sum: f64 = context
                .metrics_events
                .iter()
                .map(|m| m.memory_used_mb)
                .sum();
            let avg_memory_used = memory_sum / context.metrics_events.len() as f64;

            let energy_sum: f64 = context.metrics_events.iter().map(|m| m.energy_impact).sum();
            let avg_energy_impact = energy_sum / context.metrics_events.len() as f64;

            (
                avg_cpu_usage,
                avg_cpu_power,
                avg_gpu_usage,
                avg_gpu_power,
                avg_memory_used,
                avg_energy_impact,
            )
        } else {
            (0.0, 0.0, None, None, 0.0, 0.0)
        };

        // Format recent error logs
        let recent_errors = context
            .log_events
            .iter()
            .filter(|event| {
                matches!(
                    event.message_type,
                    crate::events::MessageType::Error | crate::events::MessageType::Fault
                )
            })
            .take(10) // Limit to most recent 10 errors to avoid overwhelming the prompt
            .map(|event| {
                format!(
                    "[{}] {}/{}: {:?} - {}",
                    event.timestamp.format("%H:%M:%S"),
                    event.subsystem,
                    event.process,
                    event.message_type,
                    event.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Format recent metrics
        let recent_metrics = context
            .metrics_events
            .iter()
            .take(5) // Limit to most recent 5 metrics samples
            .map(|event| {
                let gpu_info = match (event.gpu_usage_percent, event.gpu_power_mw) {
                    (Some(usage), Some(power)) => format!(", GPU: {:.1}% ({:.1}mW)", usage, power),
                    (Some(usage), None) => format!(", GPU: {:.1}%", usage),
                    (None, Some(power)) => format!(", GPU: {:.1}mW", power),
                    (None, None) => ", GPU: N/A".to_string(),
                };

                format!(
                    "[{}] CPU: {:.1}% ({:.1}mW){}, Memory: {:.1}MB ({:?}), Energy: {:.1}mW",
                    event.timestamp.format("%H:%M:%S"),
                    event.cpu_usage_percent,
                    event.cpu_power_mw,
                    gpu_info,
                    event.memory_used_mb,
                    event.memory_pressure,
                    event.energy_impact
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Build the complete prompt
        format!(
            r#"You are a macOS system diagnostics expert. Analyze the following system data and provide:
1. A concise summary of the issue
2. The likely root cause
3. Actionable recommendations

System Context:
- Time Window: {}
- Error Count: {}
- Fault Count: {}
- Total Log Events: {}
- Total Metrics Events: {}
- Memory Pressure: {}
- Average CPU Usage: {:.1}%
- Average CPU Power: {:.1}mW
- Average GPU Usage: {}
- Average GPU Power: {}
- Average Memory Used: {:.1}MB
- Energy Impact: {:.1}mW
- Triggered By: {}
- Trigger Reason: {}

Recent Errors:
{}

Recent Metrics:
{}

Respond in JSON format with fields: 
- summary (string): Brief description of the main issue
- root_cause (string or null): Most likely underlying cause
- recommendations (array of strings): Specific actionable steps
- severity (string): "info", "warning", or "critical"

Example response:
{{
  "summary": "High CPU usage detected in Safari processes",
  "root_cause": "Multiple tabs with heavy JavaScript execution",
  "recommendations": ["Close unused browser tabs", "Check for runaway JavaScript", "Consider using Safari's Energy tab"],
  "severity": "warning"
}}"#,
            duration,
            summary.error_count,
            summary.fault_count,
            summary.total_log_events,
            summary.total_metrics_events,
            memory_pressure,
            avg_cpu_usage,
            avg_cpu_power,
            avg_gpu_usage
                .map(|usage| format!("{:.1}%", usage))
                .unwrap_or_else(|| "N/A".to_string()),
            avg_gpu_power
                .map(|power| format!("{:.1}mW", power))
                .unwrap_or_else(|| "N/A".to_string()),
            avg_memory_used,
            avg_energy_impact,
            context.triggered_by,
            context.trigger_reason,
            if recent_errors.is_empty() {
                "No recent errors"
            } else {
                &recent_errors
            },
            if recent_metrics.is_empty() {
                "No recent metrics"
            } else {
                &recent_metrics
            }
        )
    }
}

impl AIInsight {
    /// Create a new AI insight
    pub fn new(
        summary: String,
        root_cause: Option<String>,
        recommendations: Vec<String>,
        severity: Severity,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            summary,
            root_cause,
            recommendations,
            severity,
        }
    }

    /// Check if this insight requires immediate attention
    pub fn is_critical(&self) -> bool {
        self.severity == Severity::Critical
    }

    /// Get a formatted summary for notifications
    pub fn notification_summary(&self) -> String {
        if let Some(ref cause) = self.root_cause {
            format!("{} ({})", self.summary, cause)
        } else {
            self.summary.clone()
        }
    }

    /// Get the notification title (summary)
    pub fn notification_title(&self) -> &str {
        &self.summary
    }

    /// Get the notification body (recommendations)
    pub fn notification_body(&self) -> String {
        if self.recommendations.is_empty() {
            "No specific recommendations available.".to_string()
        } else {
            self.recommendations.join("; ")
        }
    }
}

/// Placeholder backend for testing and development
///
/// This backend generates synthetic insights without requiring
/// an actual LLM connection. Used when no real backend is configured.
struct PlaceholderBackend;

unsafe impl Send for PlaceholderBackend {}
unsafe impl Sync for PlaceholderBackend {}

impl LLMBackend for PlaceholderBackend {
    fn analyze<'a>(
        &'a self,
        _context: &'a TriggerContext,
    ) -> Pin<Box<dyn Future<Output = Result<AIInsight, AnalysisError>> + Send + 'a>> {
        Box::pin(async move {
            Ok(AIInsight::new(
                "System Analysis Placeholder".to_string(),
                Some(
                    "AI analysis is not yet configured. Please set up an LLM backend.".to_string(),
                ),
                vec!["Configure Ollama or OpenAI backend".to_string()],
                Severity::Info,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{LogEvent, MemoryPressure, MessageType, MetricsEvent};
    use chrono::Utc;

    fn create_test_log_event(message_type: MessageType, message: &str) -> LogEvent {
        LogEvent {
            timestamp: Utc::now(),
            message_type,
            subsystem: "com.apple.test".to_string(),
            category: "test".to_string(),
            process: "testd".to_string(),
            process_id: 1234,
            message: message.to_string(),
        }
    }

    fn create_test_metrics_event(cpu_power: f64, memory_pressure: MemoryPressure) -> MetricsEvent {
        MetricsEvent {
            timestamp: Utc::now(),
            cpu_power_mw: cpu_power,
            cpu_usage_percent: (cpu_power / 50.0).min(100.0),
            gpu_power_mw: Some(500.0),
            gpu_usage_percent: Some(25.0),
            memory_pressure,
            memory_used_mb: 4096.0,
            energy_impact: cpu_power + 500.0,
        }
    }

    #[test]
    fn test_ai_insight_creation() {
        let insight = AIInsight::new(
            "Test Issue".to_string(),
            Some("This is a test issue".to_string()),
            vec!["Fix the issue".to_string()],
            Severity::Warning,
        );

        assert_eq!(insight.severity, Severity::Warning);
        assert_eq!(insight.summary, "Test Issue");
        assert_eq!(insight.root_cause, Some("This is a test issue".to_string()));
        assert_eq!(insight.recommendations.len(), 1);
        assert!(!insight.is_critical());
    }

    #[test]
    fn test_ai_insight_with_no_root_cause() {
        let insight = AIInsight::new(
            "Test Issue".to_string(),
            None,
            vec!["Action 1".to_string()],
            Severity::Info,
        );

        assert_eq!(insight.summary, "Test Issue");
        assert_eq!(insight.root_cause, None);
        assert_eq!(insight.recommendations.len(), 1);
        assert_eq!(insight.severity, Severity::Info);
    }

    #[test]
    fn test_ai_insight_critical_severity() {
        let insight = AIInsight::new(
            "Critical Issue".to_string(),
            Some("System failure".to_string()),
            vec!["Restart system".to_string()],
            Severity::Critical,
        );

        assert_eq!(insight.summary, "Critical Issue");
        assert_eq!(insight.root_cause, Some("System failure".to_string()));
        assert!(insight.is_critical());
    }

    #[test]
    fn test_ai_insight_notification_methods() {
        let insight_with_cause = AIInsight::new(
            "Memory Warning".to_string(),
            Some("High memory usage detected".to_string()),
            vec![
                "Close unused applications".to_string(),
                "Restart system".to_string(),
            ],
            Severity::Warning,
        );

        assert_eq!(insight_with_cause.notification_title(), "Memory Warning");
        assert_eq!(
            insight_with_cause.notification_summary(),
            "Memory Warning (High memory usage detected)"
        );
        assert_eq!(
            insight_with_cause.notification_body(),
            "Close unused applications; Restart system"
        );

        let insight_no_cause =
            AIInsight::new("System OK".to_string(), None, vec![], Severity::Info);

        assert_eq!(insight_no_cause.notification_summary(), "System OK");
        assert_eq!(
            insight_no_cause.notification_body(),
            "No specific recommendations available."
        );
    }

    #[test]
    fn test_ai_insight_serialization() {
        let insight = AIInsight::new(
            "Test Issue".to_string(),
            Some("Description".to_string()),
            vec!["Action 1".to_string(), "Action 2".to_string()],
            Severity::Critical,
        );

        let json = serde_json::to_string(&insight).unwrap();
        let deserialized: AIInsight = serde_json::from_str(&json).unwrap();

        assert_eq!(insight.severity, deserialized.severity);
        assert_eq!(insight.summary, deserialized.summary);
        assert_eq!(insight.root_cause, deserialized.root_cause);
        assert_eq!(insight.recommendations, deserialized.recommendations);
    }

    #[test]
    fn test_ai_analyzer_creation() {
        let _analyzer = AIAnalyzer::new();
        // Should create successfully with placeholder backend

        let _analyzer_default = AIAnalyzer::default();
        // Default should work the same as new()
    }

    #[tokio::test]
    async fn test_placeholder_backend_analysis() {
        let analyzer = AIAnalyzer::new();

        // Create a test trigger context
        let log_events = vec![create_test_log_event(MessageType::Error, "Test error")];
        let metrics_events = vec![create_test_metrics_event(2000.0, MemoryPressure::Warning)];
        let context = TriggerContext::for_summary(&log_events, &metrics_events);

        let result = analyzer.analyze(&context).await;
        assert!(result.is_ok());

        let insight = result.unwrap();
        assert_eq!(insight.severity, Severity::Info);
        assert!(insight.summary.contains("Placeholder"));
    }

    #[tokio::test]
    async fn test_summarize_activity() {
        let analyzer = AIAnalyzer::new();

        let log_events = vec![
            create_test_log_event(MessageType::Error, "Error 1"),
            create_test_log_event(MessageType::Fault, "Fault 1"),
        ];
        let metrics_events = vec![
            create_test_metrics_event(1500.0, MemoryPressure::Normal),
            create_test_metrics_event(2500.0, MemoryPressure::Warning),
        ];

        let result = analyzer
            .summarize_activity(&log_events, &metrics_events)
            .await;
        assert!(result.is_ok());

        let insight = result.unwrap();
        // Should use placeholder backend behavior
        assert_eq!(insight.severity, Severity::Info);
    }

    // Mock backend for testing custom behavior
    struct MockBackend {
        expected_insight: AIInsight,
    }

    unsafe impl Send for MockBackend {}
    unsafe impl Sync for MockBackend {}

    impl LLMBackend for MockBackend {
        fn analyze<'a>(
            &'a self,
            _context: &'a TriggerContext,
        ) -> Pin<Box<dyn Future<Output = Result<AIInsight, AnalysisError>> + Send + 'a>> {
            Box::pin(async move { Ok(self.expected_insight.clone()) })
        }
    }

    #[tokio::test]
    async fn test_analyzer_with_custom_backend() {
        let expected_insight = AIInsight::new(
            "Custom Analysis".to_string(),
            Some("Mock backend result".to_string()),
            vec!["Take action".to_string()],
            Severity::Critical,
        );

        let backend = Arc::new(MockBackend {
            expected_insight: expected_insight.clone(),
        });
        let analyzer = AIAnalyzer::with_backend(backend);

        let log_events = vec![create_test_log_event(MessageType::Fault, "System fault")];
        let metrics_events = vec![create_test_metrics_event(5000.0, MemoryPressure::Critical)];
        let context = TriggerContext::for_summary(&log_events, &metrics_events);

        let result = analyzer.analyze(&context).await;
        assert!(result.is_ok());

        let insight = result.unwrap();
        assert_eq!(insight.severity, expected_insight.severity);
        assert_eq!(insight.summary, expected_insight.summary);
        assert_eq!(insight.root_cause, expected_insight.root_cause);
    }

    #[test]
    fn test_format_prompt() {
        let analyzer = AIAnalyzer::new();

        let log_events = vec![
            create_test_log_event(MessageType::Error, "Test error message"),
            create_test_log_event(MessageType::Fault, "System fault occurred"),
        ];
        let metrics_events = vec![
            create_test_metrics_event(2000.0, MemoryPressure::Warning),
            create_test_metrics_event(2500.0, MemoryPressure::Critical),
        ];

        let context = TriggerContext::for_summary(&log_events, &metrics_events);
        let prompt = analyzer.format_prompt(&context);

        // Verify prompt contains expected sections
        assert!(prompt.contains("You are a macOS system diagnostics expert"));
        assert!(prompt.contains("System Context:"));
        assert!(prompt.contains("Recent Errors:"));
        assert!(prompt.contains("Recent Metrics:"));
        assert!(prompt.contains("Error Count: 1"));
        assert!(prompt.contains("Fault Count: 1"));
        assert!(prompt.contains("Memory Pressure: Critical"));
        assert!(prompt.contains("Test error message"));
        assert!(prompt.contains("System fault occurred"));
        assert!(prompt.contains("CPU: 40.0% (2000.0mW)"));
        assert!(prompt.contains("CPU: 50.0% (2500.0mW)"));
        assert!(prompt.contains("JSON format"));
    }
}

// Property-based tests
#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::events::{MemoryPressure, MessageType};
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;

    /// Helper to generate valid trigger contexts for testing
    #[derive(Debug, Clone)]
    struct ValidTriggerContextData {
        log_events: Vec<LogEvent>,
        metrics_events: Vec<MetricsEvent>,
        triggered_by: String,
        trigger_reason: String,
    }

    impl Arbitrary for ValidTriggerContextData {
        fn arbitrary(g: &mut Gen) -> Self {
            let log_count = (u8::arbitrary(g) % 10) as usize;
            let metrics_count = (u8::arbitrary(g) % 10) as usize;

            let mut log_events = Vec::new();
            for i in 0..log_count {
                let message_types = [
                    MessageType::Error,
                    MessageType::Fault,
                    MessageType::Info,
                    MessageType::Debug,
                ];
                let message_type = *g.choose(&message_types).unwrap();

                log_events.push(LogEvent {
                    timestamp: Utc::now() - chrono::Duration::seconds(i as i64),
                    message_type,
                    subsystem: format!("com.apple.test{}", i),
                    category: format!("category{}", i),
                    process: format!("process{}", i),
                    process_id: 1000 + i as u32,
                    message: format!("Test message {}", i),
                });
            }

            let mut metrics_events = Vec::new();
            for i in 0..metrics_count {
                let memory_pressures = [
                    MemoryPressure::Normal,
                    MemoryPressure::Warning,
                    MemoryPressure::Critical,
                ];
                let memory_pressure = *g.choose(&memory_pressures).unwrap();

                let cpu_power = (u16::arbitrary(g) % 5000 + 500) as f64; // 500-5500mW
                let gpu_power = if bool::arbitrary(g) {
                    Some((u16::arbitrary(g) % 8000 + 200) as f64)
                } else {
                    None
                };

                metrics_events.push(MetricsEvent {
                    timestamp: Utc::now() - chrono::Duration::seconds(i as i64),
                    cpu_power_mw: cpu_power,
                    cpu_usage_percent: (cpu_power / 50.0).min(100.0),
                    gpu_power_mw: gpu_power,
                    gpu_usage_percent: gpu_power.map(|p| (p / 100.0).min(100.0)),
                    memory_pressure,
                    memory_used_mb: (i as f64 * 1024.0) % 16384.0, // Vary memory usage
                    energy_impact: cpu_power + gpu_power.unwrap_or(0.0),
                });
            }

            Self {
                log_events,
                metrics_events,
                triggered_by: format!("TestRule{}", u8::arbitrary(g)),
                trigger_reason: format!("Test reason {}", u8::arbitrary(g)),
            }
        }
    }

    impl ValidTriggerContextData {
        fn to_trigger_context(&self) -> TriggerContext {
            TriggerContext {
                timestamp: Utc::now(),
                log_events: self.log_events.clone(),
                metrics_events: self.metrics_events.clone(),
                triggered_by: self.triggered_by.clone(),
                expected_severity: Severity::Warning,
                trigger_reason: self.trigger_reason.clone(),
            }
        }
    }

    // Feature: macos-system-observer, Property 9: Prompt formatting includes context
    // Validates: Requirements 4.1
    #[quickcheck]
    fn prop_prompt_formatting_includes_context(data: ValidTriggerContextData) -> bool {
        let analyzer = AIAnalyzer::new();
        let context = data.to_trigger_context();
        let prompt = analyzer.format_prompt(&context);

        // Property: Prompt should always contain essential sections
        let has_system_expert_intro = prompt.contains("You are a macOS system diagnostics expert");
        let has_system_context = prompt.contains("System Context:");
        let has_recent_errors = prompt.contains("Recent Errors:");
        let has_recent_metrics = prompt.contains("Recent Metrics:");
        let has_json_format = prompt.contains("JSON format");
        let has_response_fields = prompt.contains("summary")
            && prompt.contains("root_cause")
            && prompt.contains("recommendations")
            && prompt.contains("severity");

        // Property: Prompt should include trigger information
        let has_triggered_by = prompt.contains(&context.triggered_by);
        let has_trigger_reason = prompt.contains(&context.trigger_reason);

        // Property: Event counts should be accurate
        let summary = context.event_summary();
        let has_error_count = prompt.contains(&format!("Error Count: {}", summary.error_count));
        let has_fault_count = prompt.contains(&format!("Fault Count: {}", summary.fault_count));
        let has_total_log_events =
            prompt.contains(&format!("Total Log Events: {}", summary.total_log_events));
        let has_total_metrics_events = prompt.contains(&format!(
            "Total Metrics Events: {}",
            summary.total_metrics_events
        ));

        // Property: If there are error/fault events, they should appear in the errors section
        let error_fault_events: Vec<_> = context
            .log_events
            .iter()
            .filter(|e| matches!(e.message_type, MessageType::Error | MessageType::Fault))
            .collect();

        let errors_properly_included = if error_fault_events.is_empty() {
            prompt.contains("No recent errors")
        } else {
            // At least some error messages should appear in the prompt
            error_fault_events
                .iter()
                .take(5)
                .all(|event| prompt.contains(&event.message) || prompt.contains(&event.process))
        };

        // Property: If there are metrics events, they should appear in the metrics section
        let metrics_properly_included = if context.metrics_events.is_empty() {
            prompt.contains("No recent metrics")
        } else {
            // At least some CPU power values should appear
            context
                .metrics_events
                .iter()
                .take(3)
                .any(|event| prompt.contains(&format!("{:.1}mW", event.cpu_power_mw)))
        };

        // Property: Memory pressure should be included if metrics exist
        let memory_pressure_included = if context.metrics_events.is_empty() {
            prompt.contains("Memory Pressure: Unknown")
        } else {
            let latest_pressure =
                &context.metrics_events[context.metrics_events.len() - 1].memory_pressure;
            prompt.contains(&format!("Memory Pressure: {:?}", latest_pressure))
        };

        has_system_expert_intro
            && has_system_context
            && has_recent_errors
            && has_recent_metrics
            && has_json_format
            && has_response_fields
            && has_triggered_by
            && has_trigger_reason
            && has_error_count
            && has_fault_count
            && has_total_log_events
            && has_total_metrics_events
            && errors_properly_included
            && metrics_properly_included
            && memory_pressure_included
    }

    // Additional property test for prompt structure consistency
    #[quickcheck]
    fn prop_prompt_structure_consistency(data: ValidTriggerContextData) -> bool {
        let analyzer = AIAnalyzer::new();
        let context = data.to_trigger_context();
        let prompt = analyzer.format_prompt(&context);

        // Property: Prompt sections should appear in the expected order
        let system_context_pos = prompt.find("System Context:").unwrap_or(usize::MAX);
        let recent_errors_pos = prompt.find("Recent Errors:").unwrap_or(usize::MAX);
        let recent_metrics_pos = prompt.find("Recent Metrics:").unwrap_or(usize::MAX);
        let json_format_pos = prompt.find("Respond in JSON format").unwrap_or(usize::MAX);

        // All sections should be found and in the correct order
        system_context_pos < recent_errors_pos
            && recent_errors_pos < recent_metrics_pos
            && recent_metrics_pos < json_format_pos
            && json_format_pos != usize::MAX
    }

    // Property test for CPU/GPU power calculations
    #[quickcheck]
    fn prop_prompt_power_calculations(data: ValidTriggerContextData) -> bool {
        let analyzer = AIAnalyzer::new();
        let context = data.to_trigger_context();
        let prompt = analyzer.format_prompt(&context);

        if context.metrics_events.is_empty() {
            // Should show 0.0 for CPU when no metrics
            prompt.contains("Average CPU Power: 0.0mW")
        } else {
            // Calculate expected average CPU power
            let cpu_sum: f64 = context.metrics_events.iter().map(|m| m.cpu_power_mw).sum();
            let expected_avg_cpu = cpu_sum / context.metrics_events.len() as f64;

            // Should contain the calculated average (with some tolerance for floating point)
            let cpu_power_str = format!("Average CPU Power: {:.1}mW", expected_avg_cpu);
            prompt.contains(&cpu_power_str)
        }
    }

    /// Mock backend that tracks invocations for testing
    #[derive(Debug)]
    struct TrackingBackend {
        invocation_count: std::sync::Arc<std::sync::Mutex<usize>>,
        last_context: std::sync::Arc<std::sync::Mutex<Option<TriggerContext>>>,
        response: AIInsight,
    }

    impl TrackingBackend {
        fn new(response: AIInsight) -> Self {
            Self {
                invocation_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
                last_context: std::sync::Arc::new(std::sync::Mutex::new(None)),
                response,
            }
        }

        fn invocation_count(&self) -> usize {
            *self.invocation_count.lock().unwrap()
        }

        fn last_context(&self) -> Option<TriggerContext> {
            self.last_context.lock().unwrap().clone()
        }
    }

    unsafe impl Send for TrackingBackend {}
    unsafe impl Sync for TrackingBackend {}

    impl LLMBackend for TrackingBackend {
        fn analyze<'a>(
            &'a self,
            context: &'a TriggerContext,
        ) -> Pin<Box<dyn Future<Output = Result<AIInsight, AnalysisError>> + Send + 'a>> {
            Box::pin(async move {
                // Track the invocation
                *self.invocation_count.lock().unwrap() += 1;
                *self.last_context.lock().unwrap() = Some(context.clone());

                Ok(self.response.clone())
            })
        }
    }

    // Feature: macos-system-observer, Property 10: AI backend receives analysis requests
    // Validates: Requirements 4.2
    #[quickcheck]
    fn prop_backend_receives_analysis_requests(data: ValidTriggerContextData) -> bool {
        let response = AIInsight::new(
            "Test Analysis".to_string(),
            Some("Test cause".to_string()),
            vec!["Test recommendation".to_string()],
            Severity::Info,
        );

        let backend = std::sync::Arc::new(TrackingBackend::new(response.clone()));
        let analyzer = AIAnalyzer::with_backend(backend.clone());
        let context = data.to_trigger_context();

        // Perform analysis
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(analyzer.analyze(&context));

        // Property: Backend should be invoked exactly once
        let invocation_count_correct = backend.invocation_count() == 1;

        // Property: Backend should receive the correct context
        let context_received_correctly = if let Some(received_context) = backend.last_context() {
            received_context.triggered_by == context.triggered_by
                && received_context.trigger_reason == context.trigger_reason
                && received_context.log_events.len() == context.log_events.len()
                && received_context.metrics_events.len() == context.metrics_events.len()
        } else {
            false
        };

        // Property: Analysis should succeed and return the expected result
        let analysis_successful = match result {
            Ok(insight) => {
                insight.summary == response.summary
                    && insight.root_cause == response.root_cause
                    && insight.recommendations == response.recommendations
                    && insight.severity == response.severity
            }
            Err(_) => false,
        };

        invocation_count_correct && context_received_correctly && analysis_successful
    }

    // Property test for summarize_activity method
    #[quickcheck]
    fn prop_summarize_activity_backend_invocation(data: ValidTriggerContextData) -> bool {
        let response = AIInsight::new(
            "Summary Analysis".to_string(),
            None,
            vec!["Summary recommendation".to_string()],
            Severity::Info,
        );

        let backend = std::sync::Arc::new(TrackingBackend::new(response.clone()));
        let analyzer = AIAnalyzer::with_backend(backend.clone());

        // Use summarize_activity instead of direct analyze
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result =
            rt.block_on(analyzer.summarize_activity(&data.log_events, &data.metrics_events));

        // Property: Backend should be invoked exactly once
        let invocation_count_correct = backend.invocation_count() == 1;

        // Property: Backend should receive a summary context
        let context_is_summary = if let Some(received_context) = backend.last_context() {
            received_context.triggered_by == "summary"
                && received_context.trigger_reason == "Periodic system summary"
                && received_context.log_events.len() == data.log_events.len()
                && received_context.metrics_events.len() == data.metrics_events.len()
        } else {
            false
        };

        // Property: Analysis should succeed
        let analysis_successful = result.is_ok();

        invocation_count_correct && context_is_summary && analysis_successful
    }
}
