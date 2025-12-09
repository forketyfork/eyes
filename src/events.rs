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

/// Raw log entry format from `log stream --style json`
///
/// This matches the actual JSON structure output by macOS, which uses
/// different field naming conventions than our internal LogEvent structure.
#[derive(Debug, Deserialize)]
struct RawLogEntry {
    timestamp: String,
    #[serde(rename = "messageType")]
    message_type: String,
    subsystem: String,
    category: String,
    process: String,
    #[serde(rename = "processID")]
    process_id: u32,
    message: String,
}

impl LogEvent {
    /// Parse a log event from the JSON format produced by `log stream --style json`
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JSON is malformed
    /// - Required fields are missing
    /// - The timestamp cannot be parsed
    /// - The message type is not recognized
    pub fn from_json(json: &str) -> Result<Self, String> {
        let raw: RawLogEntry =
            serde_json::from_str(json).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        // Parse timestamp - macOS format is "YYYY-MM-DD HH:MM:SS.ffffff-ZZZZ"
        let timestamp = chrono::DateTime::parse_from_str(&raw.timestamp, "%Y-%m-%d %H:%M:%S%.f%z")
            .map_err(|e| format!("Failed to parse timestamp '{}': {}", raw.timestamp, e))?
            .with_timezone(&Utc);

        // Parse message type
        let message_type = match raw.message_type.to_lowercase().as_str() {
            "error" => MessageType::Error,
            "fault" => MessageType::Fault,
            "info" => MessageType::Info,
            "debug" => MessageType::Debug,
            _ => return Err(format!("Unknown message type: {}", raw.message_type)),
        };

        Ok(LogEvent {
            timestamp,
            message_type,
            subsystem: raw.subsystem,
            category: raw.category,
            process: raw.process,
            process_id: raw.process_id,
            message: raw.message,
        })
    }
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

/// Raw metrics entry format from powermetrics JSON output
#[derive(Debug, Deserialize)]
struct RawMetricsEntry {
    timestamp: String,
    cpu_usage: f64,
    memory_pressure: String,
    memory_used_gb: f64,
    gpu_usage: Option<f64>,
    energy_impact: f64,
}

impl MetricsEvent {
    /// Parse a metrics event from JSON format
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JSON is malformed
    /// - Required fields are missing
    /// - The timestamp cannot be parsed
    /// - The memory pressure level is not recognized
    pub fn from_json(json: &str) -> Result<Self, String> {
        let raw: RawMetricsEntry =
            serde_json::from_str(json).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        // Parse timestamp - ISO 8601 format
        let timestamp = chrono::DateTime::parse_from_rfc3339(&raw.timestamp)
            .map_err(|e| format!("Failed to parse timestamp '{}': {}", raw.timestamp, e))?
            .with_timezone(&Utc);

        // Parse memory pressure
        let memory_pressure = match raw.memory_pressure.to_lowercase().as_str() {
            "normal" => MemoryPressure::Normal,
            "warning" => MemoryPressure::Warning,
            "critical" => MemoryPressure::Critical,
            _ => return Err(format!("Unknown memory pressure: {}", raw.memory_pressure)),
        };

        Ok(MetricsEvent {
            timestamp,
            cpu_usage: raw.cpu_usage,
            memory_pressure,
            memory_used_gb: raw.memory_used_gb,
            gpu_usage: raw.gpu_usage,
            energy_impact: raw.energy_impact,
        })
    }
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
        assert_eq!(serde_json::to_string(&Severity::Info).unwrap(), "\"info\"");
        assert_eq!(
            serde_json::to_string(&Severity::Warning).unwrap(),
            "\"warning\""
        );
        assert_eq!(
            serde_json::to_string(&Severity::Critical).unwrap(),
            "\"critical\""
        );
    }

    #[test]
    fn test_log_event_from_json_basic() {
        let json = r#"{
            "timestamp": "2024-12-09 10:30:45.123456-0800",
            "messageType": "Error",
            "subsystem": "com.apple.Safari",
            "category": "WebProcess",
            "process": "Safari",
            "processID": 1234,
            "message": "Failed to load resource"
        }"#;

        let event = LogEvent::from_json(json).unwrap();
        assert_eq!(event.message_type, MessageType::Error);
        assert_eq!(event.subsystem, "com.apple.Safari");
        assert_eq!(event.category, "WebProcess");
        assert_eq!(event.process, "Safari");
        assert_eq!(event.process_id, 1234);
        assert_eq!(event.message, "Failed to load resource");
    }
}

// Property-based tests
#[cfg(test)]
mod property_tests {
    use super::*;
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;

    /// Arbitrary implementation for MessageType to generate random message types
    impl Arbitrary for MessageType {
        fn arbitrary(g: &mut Gen) -> Self {
            let choices = [
                MessageType::Error,
                MessageType::Fault,
                MessageType::Info,
                MessageType::Debug,
            ];
            *g.choose(&choices).unwrap()
        }
    }

    /// Helper struct to generate valid log entry data
    #[derive(Debug, Clone)]
    struct ValidLogData {
        message_type: MessageType,
        subsystem: String,
        category: String,
        process: String,
        process_id: u32,
        message: String,
    }

    impl Arbitrary for ValidLogData {
        fn arbitrary(g: &mut Gen) -> Self {
            // Generate non-empty strings for required fields
            let subsystem = if bool::arbitrary(g) {
                format!("com.apple.{}", String::arbitrary(g))
            } else {
                format!("com.example.{}", String::arbitrary(g))
            };

            let category = String::arbitrary(g);
            let process = String::arbitrary(g);
            let message = String::arbitrary(g);

            ValidLogData {
                message_type: MessageType::arbitrary(g),
                subsystem,
                category,
                process,
                process_id: u32::arbitrary(g),
                message,
            }
        }
    }

    impl ValidLogData {
        /// Convert to JSON string in the format produced by `log stream`
        fn to_json(&self) -> String {
            let message_type_str = match self.message_type {
                MessageType::Error => "Error",
                MessageType::Fault => "Fault",
                MessageType::Info => "Info",
                MessageType::Debug => "Debug",
            };

            // Use a fixed timestamp format that matches macOS output
            let timestamp = "2024-12-09 10:30:45.123456-0800";

            // Use serde_json to properly escape strings
            let subsystem_json = serde_json::to_string(&self.subsystem).unwrap();
            let category_json = serde_json::to_string(&self.category).unwrap();
            let process_json = serde_json::to_string(&self.process).unwrap();
            let message_json = serde_json::to_string(&self.message).unwrap();

            format!(
                r#"{{
                    "timestamp": "{}",
                    "messageType": "{}",
                    "subsystem": {},
                    "category": {},
                    "process": {},
                    "processID": {},
                    "message": {}
                }}"#,
                timestamp,
                message_type_str,
                subsystem_json,
                category_json,
                process_json,
                self.process_id,
                message_json
            )
        }
    }

    // Feature: macos-system-observer, Property 1: Log parsing preserves structure
    // Validates: Requirements 1.2
    #[quickcheck]
    fn prop_log_parsing_preserves_structure(data: ValidLogData) -> bool {
        // Generate JSON in the format produced by `log stream`
        let json = data.to_json();

        // Parse the JSON
        let parsed = match LogEvent::from_json(&json) {
            Ok(event) => event,
            Err(_) => return false,
        };

        // Verify all fields are preserved
        parsed.message_type == data.message_type
            && parsed.subsystem == data.subsystem
            && parsed.category == data.category
            && parsed.process == data.process
            && parsed.process_id == data.process_id
            && parsed.message == data.message
    }

    /// Arbitrary implementation for MemoryPressure to generate random memory pressure levels
    impl Arbitrary for MemoryPressure {
        fn arbitrary(g: &mut Gen) -> Self {
            let choices = [
                MemoryPressure::Normal,
                MemoryPressure::Warning,
                MemoryPressure::Critical,
            ];
            *g.choose(&choices).unwrap()
        }
    }

    /// Helper struct to generate valid metrics data
    #[derive(Debug, Clone)]
    struct ValidMetricsData {
        cpu_usage: f64,
        memory_pressure: MemoryPressure,
        memory_used_gb: f64,
        gpu_usage: Option<f64>,
        energy_impact: f64,
    }

    impl Arbitrary for ValidMetricsData {
        fn arbitrary(g: &mut Gen) -> Self {
            // Generate valid CPU usage (0-100)
            let cpu_usage = (u8::arbitrary(g) % 101) as f64;

            // Generate valid memory used (0-128 GB is reasonable)
            let memory_used_gb = (u8::arbitrary(g) as f64) / 2.0;

            // Generate optional GPU usage (0-100)
            let gpu_usage = if bool::arbitrary(g) {
                Some((u8::arbitrary(g) % 101) as f64)
            } else {
                None
            };

            // Generate energy impact (0-1000 is reasonable)
            let energy_impact = (u16::arbitrary(g) % 1001) as f64;

            ValidMetricsData {
                cpu_usage,
                memory_pressure: MemoryPressure::arbitrary(g),
                memory_used_gb,
                gpu_usage,
                energy_impact,
            }
        }
    }

    impl ValidMetricsData {
        /// Convert to JSON string in the format produced by powermetrics
        fn to_json(&self) -> String {
            let memory_pressure_str = match self.memory_pressure {
                MemoryPressure::Normal => "Normal",
                MemoryPressure::Warning => "Warning",
                MemoryPressure::Critical => "Critical",
            };

            // Use ISO 8601 timestamp format
            let timestamp = "2024-12-09T18:30:45.123456Z";

            format!(
                r#"{{
                    "timestamp": "{}",
                    "cpu_usage": {},
                    "memory_pressure": "{}",
                    "memory_used_gb": {},
                    "gpu_usage": {},
                    "energy_impact": {}
                }}"#,
                timestamp,
                self.cpu_usage,
                memory_pressure_str,
                self.memory_used_gb,
                match self.gpu_usage {
                    Some(val) => val.to_string(),
                    None => "null".to_string(),
                },
                self.energy_impact
            )
        }
    }

    // Feature: macos-system-observer, Property 4: Metrics parsing extracts all fields
    // Validates: Requirements 2.2
    #[quickcheck]
    fn prop_metrics_parsing_extracts_all_fields(data: ValidMetricsData) -> bool {
        // Generate JSON in the format produced by powermetrics
        let json = data.to_json();

        // Parse the JSON
        let parsed = match MetricsEvent::from_json(&json) {
            Ok(event) => event,
            Err(_) => return false,
        };

        // Verify all fields are extracted correctly
        (parsed.cpu_usage - data.cpu_usage).abs() < 0.001
            && parsed.memory_pressure == data.memory_pressure
            && (parsed.memory_used_gb - data.memory_used_gb).abs() < 0.001
            && parsed.gpu_usage == data.gpu_usage
            && (parsed.energy_impact - data.energy_impact).abs() < 0.001
    }
}
