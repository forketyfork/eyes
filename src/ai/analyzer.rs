use crate::ai::backends::LLMBackend;
use crate::error::AnalysisError;
use crate::events::{LogEvent, MetricsEvent, Severity, Timestamp};
use crate::triggers::TriggerContext;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// AI-powered system analysis coordinator
///
/// The AIAnalyzer receives trigger contexts containing recent system events
/// and coordinates with LLM backends to generate actionable insights.
pub struct AIAnalyzer {
    backend: Arc<dyn LLMBackend>,
}

/// AI-generated insight about system behavior
///
/// Represents the result of AI analysis, including severity assessment,
/// root cause analysis, and recommended actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AIInsight {
    /// When this insight was generated
    pub timestamp: Timestamp,
    /// Severity level for alerting and prioritization
    pub severity: Severity,
    /// Human-readable title summarizing the issue
    pub title: String,
    /// Detailed analysis of what was detected
    pub description: String,
    /// Specific actions the user can take to address the issue
    pub recommendations: Vec<String>,
    /// Confidence level in the analysis (0.0 to 1.0)
    pub confidence: f64,
    /// Tags for categorization and filtering
    pub tags: Vec<String>,
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
        }
    }

    /// Create an AI analyzer with a specific LLM backend
    pub fn with_backend(backend: Arc<dyn LLMBackend>) -> Self {
        Self { backend }
    }

    /// Analyze a trigger context and generate insights
    ///
    /// This is the main entry point for AI analysis. It takes a trigger context
    /// containing recent system events and returns actionable insights.
    ///
    /// # Errors
    ///
    /// Returns `AnalysisError` if:
    /// - The backend communication fails
    /// - The response format is invalid
    /// - A timeout occurs during analysis
    pub async fn analyze(&self, context: &TriggerContext) -> Result<AIInsight, AnalysisError> {
        // Delegate to the backend for actual analysis
        self.backend.analyze(context)
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
}

impl AIInsight {
    /// Create a new AI insight
    pub fn new(
        severity: Severity,
        title: String,
        description: String,
        recommendations: Vec<String>,
        confidence: f64,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            severity,
            title,
            description,
            recommendations,
            confidence: confidence.clamp(0.0, 1.0),
            tags: Vec::new(),
        }
    }

    /// Add tags to this insight for categorization
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Check if this insight requires immediate attention
    pub fn is_critical(&self) -> bool {
        self.severity == Severity::Critical
    }

    /// Check if this insight has high confidence (>= 0.8)
    pub fn is_high_confidence(&self) -> bool {
        self.confidence >= 0.8
    }

    /// Get a formatted summary for notifications
    pub fn notification_summary(&self) -> String {
        format!("{}: {}", self.title, self.description)
    }
}

/// Placeholder backend for testing and development
///
/// This backend generates synthetic insights without requiring
/// an actual LLM connection. Used when no real backend is configured.
struct PlaceholderBackend;

impl LLMBackend for PlaceholderBackend {
    fn analyze(&self, _context: &TriggerContext) -> Result<AIInsight, AnalysisError> {
        Ok(AIInsight::new(
            Severity::Info,
            "System Analysis Placeholder".to_string(),
            "AI analysis is not yet configured. Please set up an LLM backend.".to_string(),
            vec!["Configure Ollama or OpenAI backend".to_string()],
            0.0,
        ))
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
            gpu_power_mw: Some(500.0),
            memory_pressure,
        }
    }

    #[test]
    fn test_ai_insight_creation() {
        let insight = AIInsight::new(
            Severity::Warning,
            "Test Issue".to_string(),
            "This is a test issue".to_string(),
            vec!["Fix the issue".to_string()],
            0.85,
        );

        assert_eq!(insight.severity, Severity::Warning);
        assert_eq!(insight.title, "Test Issue");
        assert_eq!(insight.description, "This is a test issue");
        assert_eq!(insight.recommendations.len(), 1);
        assert_eq!(insight.confidence, 0.85);
        assert!(insight.is_high_confidence());
        assert!(!insight.is_critical());
    }

    #[test]
    fn test_ai_insight_confidence_clamping() {
        let insight_high = AIInsight::new(
            Severity::Info,
            "Test".to_string(),
            "Test".to_string(),
            vec![],
            1.5, // Should be clamped to 1.0
        );
        assert_eq!(insight_high.confidence, 1.0);

        let insight_low = AIInsight::new(
            Severity::Info,
            "Test".to_string(),
            "Test".to_string(),
            vec![],
            -0.5, // Should be clamped to 0.0
        );
        assert_eq!(insight_low.confidence, 0.0);
    }

    #[test]
    fn test_ai_insight_with_tags() {
        let insight = AIInsight::new(
            Severity::Critical,
            "Critical Issue".to_string(),
            "System failure".to_string(),
            vec!["Restart system".to_string()],
            0.95,
        )
        .with_tags(vec!["system".to_string(), "critical".to_string()]);

        assert_eq!(insight.tags.len(), 2);
        assert!(insight.tags.contains(&"system".to_string()));
        assert!(insight.tags.contains(&"critical".to_string()));
        assert!(insight.is_critical());
    }

    #[test]
    fn test_ai_insight_notification_summary() {
        let insight = AIInsight::new(
            Severity::Warning,
            "Memory Warning".to_string(),
            "High memory usage detected".to_string(),
            vec![],
            0.8,
        );

        let summary = insight.notification_summary();
        assert_eq!(summary, "Memory Warning: High memory usage detected");
    }

    #[test]
    fn test_ai_insight_serialization() {
        let insight = AIInsight::new(
            Severity::Critical,
            "Test Issue".to_string(),
            "Description".to_string(),
            vec!["Action 1".to_string(), "Action 2".to_string()],
            0.9,
        )
        .with_tags(vec!["test".to_string()]);

        let json = serde_json::to_string(&insight).unwrap();
        let deserialized: AIInsight = serde_json::from_str(&json).unwrap();

        assert_eq!(insight.severity, deserialized.severity);
        assert_eq!(insight.title, deserialized.title);
        assert_eq!(insight.description, deserialized.description);
        assert_eq!(insight.recommendations, deserialized.recommendations);
        assert_eq!(insight.confidence, deserialized.confidence);
        assert_eq!(insight.tags, deserialized.tags);
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
        assert!(insight.title.contains("Placeholder"));
        assert_eq!(insight.confidence, 0.0);
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

    impl LLMBackend for MockBackend {
        fn analyze(&self, _context: &TriggerContext) -> Result<AIInsight, AnalysisError> {
            Ok(self.expected_insight.clone())
        }
    }

    #[tokio::test]
    async fn test_analyzer_with_custom_backend() {
        let expected_insight = AIInsight::new(
            Severity::Critical,
            "Custom Analysis".to_string(),
            "Mock backend result".to_string(),
            vec!["Take action".to_string()],
            0.95,
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
        assert_eq!(insight.title, expected_insight.title);
        assert_eq!(insight.description, expected_insight.description);
        assert_eq!(insight.confidence, expected_insight.confidence);
    }
}
