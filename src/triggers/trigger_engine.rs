use crate::events::{DiskEvent, LogEvent, MetricsEvent, Severity, Timestamp};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Engine for evaluating trigger conditions and determining when to invoke AI analysis
pub struct TriggerEngine {
    rules: Vec<Box<dyn TriggerRule>>,
}

/// Trait for implementing trigger rules that determine when AI analysis should be invoked
pub trait TriggerRule: Send + Sync {
    /// Evaluate whether this rule is triggered by the given events
    fn evaluate(
        &self,
        log_events: &[LogEvent],
        metrics_events: &[MetricsEvent],
        disk_events: &[DiskEvent],
    ) -> bool;

    /// Get a human-readable name for this rule
    fn name(&self) -> &str;

    /// Get the severity level if this rule triggers
    fn severity(&self) -> Severity;
}

/// Context passed to AI analyzer containing recent system events and trigger information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerContext {
    /// When this trigger context was created
    pub timestamp: Timestamp,
    /// Recent log events that contributed to the trigger
    pub log_events: Vec<LogEvent>,
    /// Recent metrics events that contributed to the trigger
    pub metrics_events: Vec<MetricsEvent>,
    /// Recent disk events that contributed to the trigger
    pub disk_events: Vec<DiskEvent>,
    /// Name of the rule that triggered this analysis
    pub triggered_by: String,
    /// Expected severity level based on the trigger
    pub expected_severity: Severity,
    /// Additional context about why the trigger fired
    pub trigger_reason: String,
}

impl Default for TriggerEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TriggerEngine {
    /// Create a new trigger engine with no rules
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a trigger rule to the engine
    pub fn add_rule(&mut self, rule: Box<dyn TriggerRule>) {
        use log::info;

        info!(
            "Adding trigger rule: '{}' with severity: {:?}",
            rule.name(),
            rule.severity()
        );
        self.rules.push(rule);
    }

    /// Evaluate all rules against recent events and return trigger contexts for any that fire
    pub fn evaluate(
        &self,
        log_events: &[LogEvent],
        metrics_events: &[MetricsEvent],
        disk_events: &[DiskEvent],
    ) -> Vec<TriggerContext> {
        use log::{debug, info};

        debug!(
            "Evaluating {} trigger rules against {} log events, {} metrics events, and {} disk events",
            self.rules.len(),
            log_events.len(),
            metrics_events.len(),
            disk_events.len()
        );

        let mut contexts = Vec::new();

        for rule in &self.rules {
            debug!("Evaluating rule: '{}'", rule.name());
            if rule.evaluate(log_events, metrics_events, disk_events) {
                info!(
                    "Trigger rule '{}' activated with severity: {:?}",
                    rule.name(),
                    rule.severity()
                );

                let context = TriggerContext {
                    timestamp: Utc::now(),
                    log_events: log_events.to_vec(),
                    metrics_events: metrics_events.to_vec(),
                    disk_events: disk_events.to_vec(),
                    triggered_by: rule.name().to_string(),
                    expected_severity: rule.severity(),
                    trigger_reason: format!("Rule '{}' triggered", rule.name()),
                };
                contexts.push(context);
            } else {
                debug!("Rule '{}' did not trigger", rule.name());
            }
        }

        if contexts.is_empty() {
            debug!("No trigger rules activated");
        } else {
            info!(
                "Generated {} trigger contexts for AI analysis",
                contexts.len()
            );
        }

        contexts
    }

    /// Get the number of configured rules
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

impl TriggerContext {
    /// Create a trigger context for summary analysis (not triggered by a specific rule)
    pub fn for_summary(
        log_events: &[LogEvent],
        metrics_events: &[MetricsEvent],
        disk_events: &[DiskEvent],
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            log_events: log_events.to_vec(),
            metrics_events: metrics_events.to_vec(),
            disk_events: disk_events.to_vec(),
            triggered_by: "summary".to_string(),
            expected_severity: Severity::Info,
            trigger_reason: "Periodic system summary".to_string(),
        }
    }

    /// Get the time range covered by events in this context
    pub fn time_range(&self) -> Option<(Timestamp, Timestamp)> {
        let mut min_time = None;
        let mut max_time = None;

        // Check log events
        for event in &self.log_events {
            match (min_time, max_time) {
                (None, None) => {
                    min_time = Some(event.timestamp);
                    max_time = Some(event.timestamp);
                }
                (Some(min), Some(max)) => {
                    if event.timestamp < min {
                        min_time = Some(event.timestamp);
                    }
                    if event.timestamp > max {
                        max_time = Some(event.timestamp);
                    }
                }
                _ => unreachable!(),
            }
        }

        // Check metrics events
        for event in &self.metrics_events {
            match (min_time, max_time) {
                (None, None) => {
                    min_time = Some(event.timestamp);
                    max_time = Some(event.timestamp);
                }
                (Some(min), Some(max)) => {
                    if event.timestamp < min {
                        min_time = Some(event.timestamp);
                    }
                    if event.timestamp > max {
                        max_time = Some(event.timestamp);
                    }
                }
                _ => unreachable!(),
            }
        }

        // Check disk events
        for event in &self.disk_events {
            match (min_time, max_time) {
                (None, None) => {
                    min_time = Some(event.timestamp);
                    max_time = Some(event.timestamp);
                }
                (Some(min), Some(max)) => {
                    if event.timestamp < min {
                        min_time = Some(event.timestamp);
                    }
                    if event.timestamp > max {
                        max_time = Some(event.timestamp);
                    }
                }
                _ => unreachable!(),
            }
        }

        match (min_time, max_time) {
            (Some(min), Some(max)) => Some((min, max)),
            _ => None,
        }
    }

    /// Count events by type for analysis context
    pub fn event_summary(&self) -> EventSummary {
        let mut error_count = 0;
        let mut fault_count = 0;
        let mut info_count = 0;
        let mut debug_count = 0;

        for event in &self.log_events {
            match event.message_type {
                crate::events::MessageType::Error => error_count += 1,
                crate::events::MessageType::Fault => fault_count += 1,
                crate::events::MessageType::Info => info_count += 1,
                crate::events::MessageType::Debug => debug_count += 1,
            }
        }

        EventSummary {
            total_log_events: self.log_events.len(),
            total_metrics_events: self.metrics_events.len(),
            total_disk_events: self.disk_events.len(),
            error_count,
            fault_count,
            info_count,
            debug_count,
        }
    }
}

/// Summary statistics about events in a trigger context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSummary {
    pub total_log_events: usize,
    pub total_metrics_events: usize,
    pub total_disk_events: usize,
    pub error_count: usize,
    pub fault_count: usize,
    pub info_count: usize,
    pub debug_count: usize,
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

    // Mock trigger rule for testing
    struct MockTriggerRule {
        name: String,
        should_trigger: bool,
        severity: Severity,
    }

    impl TriggerRule for MockTriggerRule {
        fn evaluate(
            &self,
            _log_events: &[LogEvent],
            _metrics_events: &[MetricsEvent],
            _disk_events: &[DiskEvent],
        ) -> bool {
            self.should_trigger
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn severity(&self) -> Severity {
            self.severity
        }
    }

    #[test]
    fn test_trigger_engine_creation() {
        let engine = TriggerEngine::new();
        assert_eq!(engine.rule_count(), 0);

        let engine_default = TriggerEngine::default();
        assert_eq!(engine_default.rule_count(), 0);
    }

    #[test]
    fn test_trigger_engine_add_rule() {
        let mut engine = TriggerEngine::new();

        let rule = Box::new(MockTriggerRule {
            name: "test_rule".to_string(),
            should_trigger: false,
            severity: Severity::Warning,
        });

        engine.add_rule(rule);
        assert_eq!(engine.rule_count(), 1);
    }

    #[test]
    fn test_trigger_engine_evaluate_no_triggers() {
        let mut engine = TriggerEngine::new();

        let rule = Box::new(MockTriggerRule {
            name: "no_trigger_rule".to_string(),
            should_trigger: false,
            severity: Severity::Info,
        });

        engine.add_rule(rule);

        let log_events = vec![create_test_log_event(MessageType::Info, "Normal log")];
        let metrics_events = vec![create_test_metrics_event(1000.0, MemoryPressure::Normal)];

        let contexts = engine.evaluate(&log_events, &metrics_events, &[]);
        assert_eq!(contexts.len(), 0);
    }

    #[test]
    fn test_trigger_engine_evaluate_with_triggers() {
        let mut engine = TriggerEngine::new();

        let rule1 = Box::new(MockTriggerRule {
            name: "trigger_rule_1".to_string(),
            should_trigger: true,
            severity: Severity::Warning,
        });

        let rule2 = Box::new(MockTriggerRule {
            name: "trigger_rule_2".to_string(),
            should_trigger: true,
            severity: Severity::Critical,
        });

        let rule3 = Box::new(MockTriggerRule {
            name: "no_trigger_rule".to_string(),
            should_trigger: false,
            severity: Severity::Info,
        });

        engine.add_rule(rule1);
        engine.add_rule(rule2);
        engine.add_rule(rule3);

        let log_events = vec![create_test_log_event(MessageType::Error, "Error log")];
        let metrics_events = vec![create_test_metrics_event(3000.0, MemoryPressure::Warning)];

        let contexts = engine.evaluate(&log_events, &metrics_events, &[]);
        assert_eq!(contexts.len(), 2); // Only the first two rules should trigger

        // Check first context
        assert_eq!(contexts[0].triggered_by, "trigger_rule_1");
        assert_eq!(contexts[0].expected_severity, Severity::Warning);
        assert_eq!(contexts[0].log_events.len(), 1);
        assert_eq!(contexts[0].metrics_events.len(), 1);

        // Check second context
        assert_eq!(contexts[1].triggered_by, "trigger_rule_2");
        assert_eq!(contexts[1].expected_severity, Severity::Critical);
    }

    #[test]
    fn test_trigger_context_for_summary() {
        let log_events = vec![
            create_test_log_event(MessageType::Error, "Error 1"),
            create_test_log_event(MessageType::Info, "Info 1"),
        ];
        let metrics_events = vec![create_test_metrics_event(1500.0, MemoryPressure::Normal)];

        let context = TriggerContext::for_summary(&log_events, &metrics_events, &[]);

        assert_eq!(context.triggered_by, "summary");
        assert_eq!(context.expected_severity, Severity::Info);
        assert_eq!(context.trigger_reason, "Periodic system summary");
        assert_eq!(context.log_events.len(), 2);
        assert_eq!(context.metrics_events.len(), 1);
    }

    #[test]
    fn test_trigger_context_time_range() {
        let now = Utc::now();
        let earlier = now - chrono::Duration::seconds(30);
        let later = now + chrono::Duration::seconds(30);

        let mut log_event1 = create_test_log_event(MessageType::Info, "First");
        log_event1.timestamp = earlier;

        let mut log_event2 = create_test_log_event(MessageType::Info, "Second");
        log_event2.timestamp = later;

        let mut metrics_event = create_test_metrics_event(1000.0, MemoryPressure::Normal);
        metrics_event.timestamp = now;

        let context = TriggerContext::for_summary(&[log_event1, log_event2], &[metrics_event], &[]);

        let time_range = context.time_range();
        assert!(time_range.is_some());

        let (min_time, max_time) = time_range.unwrap();
        assert_eq!(min_time, earlier);
        assert_eq!(max_time, later);
    }

    #[test]
    fn test_trigger_context_time_range_empty() {
        let context = TriggerContext::for_summary(&[], &[], &[]);
        assert!(context.time_range().is_none());
    }

    #[test]
    fn test_trigger_context_event_summary() {
        let log_events = vec![
            create_test_log_event(MessageType::Error, "Error 1"),
            create_test_log_event(MessageType::Error, "Error 2"),
            create_test_log_event(MessageType::Fault, "Fault 1"),
            create_test_log_event(MessageType::Info, "Info 1"),
            create_test_log_event(MessageType::Debug, "Debug 1"),
        ];
        let metrics_events = vec![
            create_test_metrics_event(1000.0, MemoryPressure::Normal),
            create_test_metrics_event(2000.0, MemoryPressure::Warning),
        ];

        let context = TriggerContext::for_summary(&log_events, &metrics_events, &[]);
        let summary = context.event_summary();

        assert_eq!(summary.total_log_events, 5);
        assert_eq!(summary.total_metrics_events, 2);
        assert_eq!(summary.total_disk_events, 0);
        assert_eq!(summary.error_count, 2);
        assert_eq!(summary.fault_count, 1);
        assert_eq!(summary.info_count, 1);
        assert_eq!(summary.debug_count, 1);
    }

    #[test]
    fn test_trigger_context_serialization() {
        let log_events = vec![create_test_log_event(MessageType::Fault, "Test fault")];
        let metrics_events = vec![create_test_metrics_event(1500.0, MemoryPressure::Normal)];

        let context = TriggerContext::for_summary(&log_events, &metrics_events, &[]);

        let json = serde_json::to_string(&context).unwrap();
        let deserialized: TriggerContext = serde_json::from_str(&json).unwrap();

        assert_eq!(context.triggered_by, deserialized.triggered_by);
        assert_eq!(context.expected_severity, deserialized.expected_severity);
        assert_eq!(context.trigger_reason, deserialized.trigger_reason);
        assert_eq!(context.log_events.len(), deserialized.log_events.len());
        assert_eq!(
            context.metrics_events.len(),
            deserialized.metrics_events.len()
        );
    }

    #[test]
    fn test_event_summary_serialization() {
        let summary = EventSummary {
            total_log_events: 10,
            total_metrics_events: 5,
            total_disk_events: 0,
            error_count: 2,
            fault_count: 1,
            info_count: 6,
            debug_count: 1,
        };

        let json = serde_json::to_string(&summary).unwrap();
        let deserialized: EventSummary = serde_json::from_str(&json).unwrap();

        assert_eq!(summary.total_log_events, deserialized.total_log_events);
        assert_eq!(
            summary.total_metrics_events,
            deserialized.total_metrics_events
        );
        assert_eq!(summary.error_count, deserialized.error_count);
        assert_eq!(summary.fault_count, deserialized.fault_count);
        assert_eq!(summary.info_count, deserialized.info_count);
        assert_eq!(summary.debug_count, deserialized.debug_count);
    }
}
// Property-based tests
#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::events::{MemoryPressure, MessageType};
    use crate::triggers::rules::{ErrorFrequencyRule, MemoryPressureRule, ResourceSpikeRule};
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;

    /// Helper to generate log events with controlled error counts
    #[derive(Debug, Clone)]
    struct ErrorEventScenario {
        /// Number of error/fault events to generate
        error_count: usize,
        /// Number of non-error events to generate
        non_error_count: usize,
        /// Time spread for events (in seconds)
        time_spread_seconds: i64,
    }

    impl Arbitrary for ErrorEventScenario {
        fn arbitrary(g: &mut Gen) -> Self {
            Self {
                error_count: (u8::arbitrary(g) % 20) as usize,
                non_error_count: (u8::arbitrary(g) % 20) as usize,
                time_spread_seconds: (u8::arbitrary(g) % 120 + 1) as i64, // 1-120 seconds
            }
        }
    }

    impl ErrorEventScenario {
        fn generate_log_events(&self) -> Vec<LogEvent> {
            let mut events = Vec::new();
            let now = Utc::now();
            let window = self.time_spread_seconds.max(1);

            // Generate error/fault events within the last 30 seconds to ensure they're in time window
            for i in 0..self.error_count {
                let message_type = if i % 2 == 0 {
                    MessageType::Error
                } else {
                    MessageType::Fault
                };

                // Spread events within the last 30 seconds to ensure they're captured by time windows
                let offset = (i as i64 * window) / (self.error_count.max(1) as i64);

                events.push(LogEvent {
                    timestamp: now - chrono::Duration::seconds(offset),
                    message_type,
                    subsystem: "com.apple.test".to_string(),
                    category: "test".to_string(),
                    process: "testd".to_string(),
                    process_id: 1234,
                    message: format!("Error message {}", i),
                });
            }

            // Generate non-error events within the same time window
            for i in 0..self.non_error_count {
                let message_type = if i % 2 == 0 {
                    MessageType::Info
                } else {
                    MessageType::Debug
                };

                let offset = (i as i64 * window) / (self.non_error_count.max(1) as i64);

                events.push(LogEvent {
                    timestamp: now - chrono::Duration::seconds(offset),
                    message_type,
                    subsystem: "com.apple.test".to_string(),
                    category: "test".to_string(),
                    process: "testd".to_string(),
                    process_id: 1234,
                    message: format!("Info message {}", i),
                });
            }

            events
        }
    }

    /// Helper to generate metrics events with controlled memory pressure
    #[derive(Debug, Clone)]
    struct MemoryPressureScenario {
        /// Number of events with normal memory pressure
        normal_count: usize,
        /// Number of events with warning memory pressure
        warning_count: usize,
        /// Number of events with critical memory pressure
        critical_count: usize,
    }

    impl Arbitrary for MemoryPressureScenario {
        fn arbitrary(g: &mut Gen) -> Self {
            Self {
                normal_count: (u8::arbitrary(g) % 10) as usize,
                warning_count: (u8::arbitrary(g) % 10) as usize,
                critical_count: (u8::arbitrary(g) % 10) as usize,
            }
        }
    }

    impl MemoryPressureScenario {
        fn generate_metrics_events(&self) -> Vec<MetricsEvent> {
            let mut events = Vec::new();
            let now = Utc::now();

            // Generate normal pressure events
            for i in 0..self.normal_count {
                let cpu_power = 1000.0 + (i as f64 * 100.0);
                let gpu_power = 500.0 + (i as f64 * 50.0);
                events.push(MetricsEvent {
                    timestamp: now - chrono::Duration::seconds(i as i64),
                    cpu_power_mw: cpu_power,
                    cpu_usage_percent: (cpu_power / 50.0).min(100.0),
                    gpu_power_mw: Some(gpu_power),
                    gpu_usage_percent: Some((gpu_power / 100.0).min(100.0)),
                    memory_pressure: MemoryPressure::Normal,
                    memory_used_mb: 2048.0 + (i as f64 * 512.0),
                    energy_impact: cpu_power + gpu_power,
                });
            }

            // Generate warning pressure events
            for i in 0..self.warning_count {
                let cpu_power = 1500.0 + (i as f64 * 100.0);
                let gpu_power = 800.0 + (i as f64 * 50.0);
                events.push(MetricsEvent {
                    timestamp: now - chrono::Duration::seconds((self.normal_count + i) as i64),
                    cpu_power_mw: cpu_power,
                    cpu_usage_percent: (cpu_power / 50.0).min(100.0),
                    gpu_power_mw: Some(gpu_power),
                    gpu_usage_percent: Some((gpu_power / 100.0).min(100.0)),
                    memory_pressure: MemoryPressure::Warning,
                    memory_used_mb: 6144.0 + (i as f64 * 512.0),
                    energy_impact: cpu_power + gpu_power,
                });
            }

            // Generate critical pressure events
            for i in 0..self.critical_count {
                let cpu_power = 2000.0 + (i as f64 * 100.0);
                let gpu_power = 1200.0 + (i as f64 * 50.0);
                events.push(MetricsEvent {
                    timestamp: now
                        - chrono::Duration::seconds(
                            (self.normal_count + self.warning_count + i) as i64,
                        ),
                    cpu_power_mw: cpu_power,
                    cpu_usage_percent: (cpu_power / 50.0).min(100.0),
                    gpu_power_mw: Some(gpu_power),
                    gpu_usage_percent: Some((gpu_power / 100.0).min(100.0)),
                    memory_pressure: MemoryPressure::Critical,
                    memory_used_mb: 12288.0 + (i as f64 * 512.0),
                    energy_impact: cpu_power + gpu_power,
                });
            }

            events
        }

        fn has_warning_or_critical(&self) -> bool {
            self.warning_count > 0 || self.critical_count > 0
        }

        fn has_critical(&self) -> bool {
            self.critical_count > 0
        }
    }

    /// Helper to generate resource spike scenarios
    #[derive(Debug, Clone)]
    struct ResourceSpikeScenario {
        /// Initial CPU power (milliwatts)
        initial_cpu_mw: f64,
        /// CPU power increase (milliwatts)
        cpu_increase_mw: f64,
        /// Initial GPU power (milliwatts, optional)
        initial_gpu_mw: Option<f64>,
        /// GPU power increase (milliwatts, only if initial_gpu_mw is Some)
        gpu_increase_mw: f64,
        /// Time between measurements (seconds)
        time_gap_seconds: i64,
    }

    impl Arbitrary for ResourceSpikeScenario {
        fn arbitrary(g: &mut Gen) -> Self {
            let initial_cpu_mw = (u16::arbitrary(g) % 5000 + 500) as f64; // 500-5500mW
            let cpu_increase_mw = (u16::arbitrary(g) % 3000) as f64; // 0-3000mW increase

            let initial_gpu_mw = if bool::arbitrary(g) {
                Some((u16::arbitrary(g) % 8000 + 200) as f64) // 200-8200mW
            } else {
                None
            };

            let gpu_increase_mw = (u16::arbitrary(g) % 5000) as f64; // 0-5000mW increase
            let time_gap_seconds = (u8::arbitrary(g) % 60 + 1) as i64; // 1-60 seconds

            Self {
                initial_cpu_mw,
                cpu_increase_mw,
                initial_gpu_mw,
                gpu_increase_mw,
                time_gap_seconds,
            }
        }
    }

    impl ResourceSpikeScenario {
        fn generate_metrics_events(&self) -> Vec<MetricsEvent> {
            let now = Utc::now();

            // Ensure events are within a reasonable time window (last 30 seconds)
            let time_gap = self.time_gap_seconds.min(30);

            let earlier_event = MetricsEvent {
                timestamp: now - chrono::Duration::seconds(time_gap),
                cpu_power_mw: self.initial_cpu_mw,
                cpu_usage_percent: (self.initial_cpu_mw / 50.0).min(100.0),
                gpu_power_mw: self.initial_gpu_mw,
                gpu_usage_percent: self.initial_gpu_mw.map(|p| (p / 100.0).min(100.0)),
                memory_pressure: MemoryPressure::Normal,
                memory_used_mb: 4096.0,
                energy_impact: self.initial_cpu_mw + self.initial_gpu_mw.unwrap_or(0.0),
            };

            let later_gpu_power = self
                .initial_gpu_mw
                .map(|initial| initial + self.gpu_increase_mw);

            let final_cpu_power = self.initial_cpu_mw + self.cpu_increase_mw;
            let later_event = MetricsEvent {
                timestamp: now - chrono::Duration::seconds(1), // 1 second ago to ensure it's recent
                cpu_power_mw: final_cpu_power,
                cpu_usage_percent: (final_cpu_power / 50.0).min(100.0),
                gpu_power_mw: later_gpu_power,
                gpu_usage_percent: later_gpu_power.map(|p| (p / 100.0).min(100.0)),
                memory_pressure: MemoryPressure::Normal,
                memory_used_mb: 4096.0,
                energy_impact: final_cpu_power + later_gpu_power.unwrap_or(0.0),
            };

            vec![earlier_event, later_event]
        }

        fn should_trigger_cpu_spike(&self, threshold_mw: f64) -> bool {
            self.cpu_increase_mw >= threshold_mw
        }

        fn should_trigger_gpu_spike(&self, threshold_mw: f64) -> bool {
            self.initial_gpu_mw.is_some() && self.gpu_increase_mw >= threshold_mw
        }

        fn should_trigger_any_spike(&self, cpu_threshold_mw: f64, gpu_threshold_mw: f64) -> bool {
            self.should_trigger_cpu_spike(cpu_threshold_mw)
                || self.should_trigger_gpu_spike(gpu_threshold_mw)
        }
    }

    // Feature: macos-system-observer, Property 8: Trigger activation on threshold breach
    // Validates: Requirements 3.3, 3.4
    #[quickcheck]
    fn prop_error_frequency_trigger_activation(scenario: ErrorEventScenario) -> bool {
        let threshold = 3;
        let window_seconds = 60;
        let rule = ErrorFrequencyRule::new(threshold, window_seconds, Severity::Warning);

        let log_events = scenario.generate_log_events();
        let metrics_events = vec![];

        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(window_seconds);
        let errors_in_window = log_events
            .iter()
            .filter(|event| {
                event.timestamp >= cutoff
                    && (event.message_type == MessageType::Error
                        || event.message_type == MessageType::Fault)
            })
            .count();
        let should_trigger = errors_in_window > threshold;
        let actually_triggers = rule.evaluate(&log_events, &metrics_events, &[]);

        // Property: Rule should trigger if and only if error count exceeds threshold
        should_trigger == actually_triggers
    }

    // Feature: macos-system-observer, Property 8b: Memory pressure trigger activation
    // Validates: Requirements 3.3, 3.4
    #[quickcheck]
    fn prop_memory_pressure_trigger_activation(scenario: MemoryPressureScenario) -> bool {
        // Test warning level rule
        let warning_rule = MemoryPressureRule::new(MemoryPressure::Warning, Severity::Warning);
        let critical_rule = MemoryPressureRule::new(MemoryPressure::Critical, Severity::Critical);

        let log_events = vec![];
        let metrics_events = scenario.generate_metrics_events();

        let warning_should_trigger = scenario.has_warning_or_critical();
        let warning_actually_triggers = warning_rule.evaluate(&log_events, &metrics_events, &[]);

        let critical_should_trigger = scenario.has_critical();
        let critical_actually_triggers = critical_rule.evaluate(&log_events, &metrics_events, &[]);

        // Property: Warning rule triggers on Warning or Critical, Critical rule only on Critical
        (warning_should_trigger == warning_actually_triggers)
            && (critical_should_trigger == critical_actually_triggers)
    }

    // Feature: macos-system-observer, Property 8c: Resource spike trigger activation
    // Validates: Requirements 3.3, 3.4
    #[quickcheck]
    fn prop_resource_spike_trigger_activation(scenario: ResourceSpikeScenario) -> bool {
        let cpu_threshold = 1000.0;
        let gpu_threshold = 2000.0;
        let window_seconds = 60;

        let rule = ResourceSpikeRule::new(
            cpu_threshold,
            gpu_threshold,
            window_seconds,
            Severity::Warning,
        );

        let log_events = vec![];
        let metrics_events = scenario.generate_metrics_events();

        let should_trigger = scenario.should_trigger_any_spike(cpu_threshold, gpu_threshold);
        let actually_triggers = rule.evaluate(&log_events, &metrics_events, &[]);

        // Property: Rule should trigger if and only if CPU or GPU spike exceeds threshold
        should_trigger == actually_triggers
    }

    // Feature: macos-system-observer, Property 8d: Trigger engine evaluation consistency
    // Validates: Requirements 3.3, 3.4
    #[quickcheck]
    fn prop_trigger_engine_evaluation_consistency(
        error_scenario: ErrorEventScenario,
        memory_scenario: MemoryPressureScenario,
    ) -> bool {
        let mut engine = TriggerEngine::new();

        // Add rules with known thresholds
        let error_threshold = 2;
        let error_rule = Box::new(ErrorFrequencyRule::new(
            error_threshold,
            60,
            Severity::Warning,
        ));
        let memory_rule = Box::new(MemoryPressureRule::new(
            MemoryPressure::Warning,
            Severity::Warning,
        ));

        engine.add_rule(error_rule);
        engine.add_rule(memory_rule);

        let log_events = error_scenario.generate_log_events();
        let metrics_events = memory_scenario.generate_metrics_events();

        let contexts = engine.evaluate(&log_events, &metrics_events, &[]);

        // Calculate expected triggers
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(60);
        let errors_in_window = log_events
            .iter()
            .filter(|event| {
                event.timestamp >= cutoff
                    && (event.message_type == MessageType::Error
                        || event.message_type == MessageType::Fault)
            })
            .count();
        let error_should_trigger = errors_in_window > error_threshold;
        let memory_should_trigger = memory_scenario.has_warning_or_critical();
        let expected_trigger_count = (if error_should_trigger { 1 } else { 0 })
            + (if memory_should_trigger { 1 } else { 0 });

        // Property: Number of trigger contexts should match expected triggers
        let contexts_match = contexts.len() == expected_trigger_count;

        // Property: Each context should contain the same events that were evaluated
        let events_preserved = contexts.iter().all(|ctx| {
            ctx.log_events.len() == log_events.len()
                && ctx.metrics_events.len() == metrics_events.len()
        });

        // Property: Triggered rule names should be correct
        let rule_names_correct = contexts.iter().all(|ctx| {
            ctx.triggered_by == "ErrorFrequencyRule" || ctx.triggered_by == "MemoryPressureRule"
        });

        contexts_match && events_preserved && rule_names_correct
    }
}
