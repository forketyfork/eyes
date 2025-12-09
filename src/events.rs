//! Core event types and enums for the macOS System Observer
//!
//! This module defines the fundamental data structures used throughout the application
//! for representing log events, metrics events, and related types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Timestamp type for consistent time handling across the application
pub type Timestamp = DateTime<Utc>;

/// Log event from the macOS Unified Log System
///
/// Represents a single log entry captured from `log stream` with all relevant metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEvent {
    /// When the log entry was created
    pub timestamp: Timestamp,
    /// Type of log message (error, fault, info, debug)
    pub message_type: MessageType,
    /// macOS subsystem that generated the log
    pub subsystem: String,
    /// Category within the subsystem
    pub category: String,
    /// Name of the process that generated the log
    pub process: String,
    /// Process ID
    pub process_id: u32,
    /// The actual log message content
    pub message: String,
}

/// Type of log message from the Unified Log System
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    /// Error-level message indicating a problem
    Error,
    /// Fault-level message indicating a serious issue
    Fault,
    /// Informational message
    Info,
    /// Debug-level message
    Debug,
}

/// Metrics snapshot from system resource monitoring
///
/// Represents a point-in-time measurement of system resource usage,
/// typically gathered from `powermetrics` or similar tools.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsEvent {
    /// When the metrics were sampled
    pub timestamp: Timestamp,
    /// CPU usage as a percentage (0-100)
    pub cpu_usage: f64,
    /// Current memory pressure level
    pub memory_pressure: MemoryPressure,
    /// Memory used in gigabytes
    pub memory_used_gb: f64,
    /// GPU usage as a percentage (0-100), None if unavailable
    pub gpu_usage: Option<f64>,
    /// Energy impact in arbitrary units from powermetrics
    pub energy_impact: f64,
}

/// Memory pressure levels from macOS memory management
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "PascalCase")]
pub enum MemoryPressure {
    /// Normal memory conditions
    Normal,
    /// Memory pressure warning - system is under some pressure
    Warning,
    /// Critical memory pressure - system may start killing processes
    Critical,
}

/// Severity level for AI-generated insights and alerts
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational insight, no action required
    Info,
    /// Warning that may require attention
    Warning,
    /// Critical issue requiring immediate attention
    Critical,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_log_event_serialization() {
        let event = LogEvent {
            timestamp: Utc::now(),
            message_type: MessageType::Error,
            subsystem: "com.apple.test".to_string(),
            category: "test".to_string(),
            process: "testd".to_string(),
            process_id: 1234,
            message: "Test error message".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: LogEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_metrics_event_serialization() {
        let event = MetricsEvent {
            timestamp: Utc::now(),
            cpu_usage: 45.5,
            memory_pressure: MemoryPressure::Warning,
            memory_used_gb: 8.2,
            gpu_usage: Some(30.0),
            energy_impact: 100.5,
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: MetricsEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_message_type_serialization() {
        assert_eq!(
            serde_json::to_string(&MessageType::Error).unwrap(),
            "\"error\""
        );
        assert_eq!(
            serde_json::to_string(&MessageType::Fault).unwrap(),
            "\"fault\""
        );
        assert_eq!(
            serde_json::to_string(&MessageType::Info).unwrap(),
            "\"info\""
        );
        assert_eq!(
            serde_json::to_string(&MessageType::Debug).unwrap(),
            "\"debug\""
        );
    }

    #[test]
    fn test_memory_pressure_ordering() {
        assert!(MemoryPressure::Normal < MemoryPressure::Warning);
        assert!(MemoryPressure::Warning < MemoryPressure::Critical);
        assert!(MemoryPressure::Normal < MemoryPressure::Critical);
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Critical);
        assert!(Severity::Info < Severity::Critical);
    }

    #[test]
    fn test_memory_pressure_serialization() {
        assert_eq!(
            serde_json::to_string(&MemoryPressure::Normal).unwrap(),
            "\"Normal\""
        );
        assert_eq!(
            serde_json::to_string(&MemoryPressure::Warning).unwrap(),
            "\"Warning\""
        );
        assert_eq!(
            serde_json::to_string(&MemoryPressure::Critical).unwrap(),
            "\"Critical\""
        );
    }

    #[test]
    fn test_severity_serialization() {
        assert_eq!(
            serde_json::to_string(&Severity::Info).unwrap(),
            "\"info\""
        );
        assert_eq!(
            serde_json::to_string(&Severity::Warning).unwrap(),
            "\"warning\""
        );
        assert_eq!(
            serde_json::to_string(&Severity::Critical).unwrap(),
            "\"critical\""
        );
    }
}
