use crate::events::{LogEvent, MetricsEvent, Severity, Timestamp};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Engine for evaluating trigger conditions and determining when to invoke AI analysis
pub struct TriggerEngine {
    rules: Vec<Box<dyn TriggerRule>>,
}

/// Trait for implementing trigger rules that determine when AI analysis should be invoked
pub trait TriggerRule: Send + Sync {
    /// Evaluate whether this rule is triggered by the given events
    fn evaluate(&self, log_events: &[LogEvent], metrics_events: &[MetricsEvent]) -> bool;

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
        self.rules.push(rule);
    }

    /// Evaluate all rules against recent events and return trigger contexts for any that fire
    pub fn evaluate(
        &self,
        log_events: &[LogEvent],
        metrics_events: &[MetricsEvent],
    ) -> Vec<TriggerContext> {
        let mut contexts = Vec::new();

        for rule in &self.rules {
            if rule.evaluate(log_events, metrics_events) {
                let context = TriggerContext {
                    timestamp: Utc::now(),
                    log_events: log_events.to_vec(),
                    metrics_events: metrics_events.to_vec(),
                    triggered_by: rule.name().to_string(),
                    expected_severity: rule.severity(),
                    trigger_reason: format!("Rule '{}' triggered", rule.name()),
                };
                contexts.push(context);
            }
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
    pub fn for_summary(log_events: &[LogEvent], metrics_events: &[MetricsEvent]) -> Self {
        Self {
            timestamp: Utc::now(),
            log_events: log_events.to_vec(),
            metrics_events: metrics_events.to_vec(),
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
            gpu_power_mw: Some(500.0),
            memory_pressure,
        }
    }

    // Mock trigger rule for testing
    struct MockTriggerRule {
        name: String,
        should_trigger: bool,
        severity: Severity,
    }

    impl TriggerRule for MockTriggerRule {
        fn evaluate(&self, _log_events: &[LogEvent], _metrics_events: &[MetricsEvent]) -> bool {
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

        let contexts = engine.evaluate(&log_events, &metrics_events);
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

        let contexts = engine.evaluate(&log_events, &metrics_events);
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

        let context = TriggerContext::for_summary(&log_events, &metrics_events);

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

        let context = TriggerContext::for_summary(&[log_event1, log_event2], &[metrics_event]);

        let time_range = context.time_range();
        assert!(time_range.is_some());

        let (min_time, max_time) = time_range.unwrap();
        assert_eq!(min_time, earlier);
        assert_eq!(max_time, later);
    }

    #[test]
    fn test_trigger_context_time_range_empty() {
        let context = TriggerContext::for_summary(&[], &[]);
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

        let context = TriggerContext::for_summary(&log_events, &metrics_events);
        let summary = context.event_summary();

        assert_eq!(summary.total_log_events, 5);
        assert_eq!(summary.total_metrics_events, 2);
        assert_eq!(summary.error_count, 2);
        assert_eq!(summary.fault_count, 1);
        assert_eq!(summary.info_count, 1);
        assert_eq!(summary.debug_count, 1);
    }

    #[test]
    fn test_trigger_context_serialization() {
        let log_events = vec![create_test_log_event(MessageType::Fault, "Test fault")];
        let metrics_events = vec![create_test_metrics_event(1500.0, MemoryPressure::Normal)];

        let context = TriggerContext::for_summary(&log_events, &metrics_events);

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
