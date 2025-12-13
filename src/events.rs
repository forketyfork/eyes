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
    /// CPU power consumption in milliwatts
    pub cpu_power_mw: f64,
    /// CPU usage percentage (0.0 to 100.0)
    pub cpu_usage_percent: f64,
    /// GPU power consumption in milliwatts, None if unavailable
    pub gpu_power_mw: Option<f64>,
    /// GPU usage percentage (0.0 to 100.0), None if unavailable
    pub gpu_usage_percent: Option<f64>,
    /// Current memory pressure level (derived from system state)
    pub memory_pressure: MemoryPressure,
    /// Memory usage in megabytes
    pub memory_used_mb: f64,
    /// Energy impact score (derived from CPU and GPU power consumption)
    pub energy_impact: f64,
}

impl MetricsEvent {
    /// Parse a metrics event from powermetrics plist format
    ///
    /// Expected plist structure:
    /// ```xml
    /// <dict>
    ///   <key>processor</key>
    ///   <dict>
    ///     <key>cpu_power</key>
    ///     <real>1234.5</real>
    ///   </dict>
    ///   <key>gpu</key>
    ///   <dict>
    ///     <key>gpu_power</key>
    ///     <real>567.8</real>
    ///   </dict>
    /// </dict>
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The plist is malformed
    /// - Required fields are missing
    /// - Values cannot be parsed as numbers
    pub fn from_plist(plist_data: &[u8]) -> Result<Self, String> {
        use plist::Value;

        let value: Value =
            plist::from_bytes(plist_data).map_err(|e| format!("Failed to parse plist: {}", e))?;

        let dict = value
            .as_dictionary()
            .ok_or_else(|| "Root plist value is not a dictionary".to_string())?;

        // Extract CPU power and usage
        let processor_dict = dict
            .get("processor")
            .and_then(|v| v.as_dictionary())
            .ok_or_else(|| "Missing processor section".to_string())?;

        let cpu_power_mw = processor_dict
            .get("cpu_power")
            .and_then(|v| v.as_real())
            .ok_or_else(|| "Missing or invalid processor.cpu_power".to_string())?;

        let cpu_usage_percent = processor_dict
            .get("cpu_usage")
            .and_then(|v| v.as_real())
            .unwrap_or_else(|| {
                // Estimate CPU usage from power consumption (rough approximation)
                // Typical laptop CPU: 1000-5000mW range maps to 0-100% usage
                (cpu_power_mw / 50.0).clamp(0.0, 100.0)
            });

        // Extract GPU power and usage (optional)
        let (gpu_power_mw, gpu_usage_percent) =
            if let Some(gpu_dict) = dict.get("gpu").and_then(|v| v.as_dictionary()) {
                let power = gpu_dict.get("gpu_power").and_then(|v| v.as_real());
                let usage = gpu_dict
                    .get("gpu_usage")
                    .and_then(|v| v.as_real())
                    .or_else(|| {
                        // Estimate GPU usage from power if available
                        power.map(|p| (p / 100.0).clamp(0.0, 100.0))
                    });
                (power, usage)
            } else {
                (None, None)
            };

        // Extract memory information from powermetrics output
        let (memory_pressure, memory_used_mb) =
            if let Some(memory_dict) = dict.get("memory").and_then(|v| v.as_dictionary()) {
                let pressure = memory_dict
                    .get("memory_pressure")
                    .and_then(|v| v.as_string())
                    .map(|s| match s.to_lowercase().as_str() {
                        "critical" => MemoryPressure::Critical,
                        "warning" => MemoryPressure::Warning,
                        _ => MemoryPressure::Normal,
                    })
                    .unwrap_or_else(|| {
                        // Derive from free memory if pressure not available
                        memory_dict
                            .get("free_memory_mb")
                            .and_then(|v| v.as_real())
                            .map(|free_mb| {
                                if free_mb < 500.0 {
                                    MemoryPressure::Critical
                                } else if free_mb < 2000.0 {
                                    MemoryPressure::Warning
                                } else {
                                    MemoryPressure::Normal
                                }
                            })
                            .unwrap_or(MemoryPressure::Normal)
                    });

                let used_mb = memory_dict
                    .get("used_memory_mb")
                    .and_then(|v| v.as_real())
                    .or_else(|| {
                        // Calculate from total - free if available
                        let total = memory_dict.get("total_memory_mb").and_then(|v| v.as_real());
                        let free = memory_dict.get("free_memory_mb").and_then(|v| v.as_real());
                        match (total, free) {
                            (Some(t), Some(f)) => Some(t - f),
                            _ => None,
                        }
                    })
                    .unwrap_or(0.0);

                (pressure, used_mb)
            } else {
                (MemoryPressure::Normal, 0.0)
            };

        // Calculate energy impact from CPU and GPU power
        let energy_impact = cpu_power_mw + gpu_power_mw.unwrap_or(0.0);

        Ok(MetricsEvent {
            timestamp: Utc::now(),
            cpu_power_mw,
            cpu_usage_percent,
            gpu_power_mw,
            gpu_usage_percent,
            memory_pressure,
            memory_used_mb,
            energy_impact,
        })
    }

    /// Parse a metrics event from JSON format (for testing/alternative sources)
    ///
    /// This is a convenience method for testing and alternative data sources.
    /// The primary parsing method is `from_plist`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JSON is malformed
    /// - Required fields are missing
    pub fn from_json(json: &str) -> Result<Self, String> {
        #[derive(Debug, Deserialize)]
        struct RawMetricsEntry {
            #[serde(default = "chrono::Utc::now")]
            timestamp: Timestamp,
            cpu_power_mw: f64,
            #[serde(default)]
            cpu_usage_percent: f64,
            gpu_power_mw: Option<f64>,
            #[serde(default)]
            gpu_usage_percent: Option<f64>,
            #[serde(default)]
            memory_pressure: String,
            #[serde(default)]
            memory_used_mb: f64,
            #[serde(default)]
            energy_impact: f64,
        }

        let raw: RawMetricsEntry =
            serde_json::from_str(json).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        // Parse memory pressure
        let memory_pressure = if raw.memory_pressure.is_empty() {
            MemoryPressure::Normal
        } else {
            match raw.memory_pressure.to_lowercase().as_str() {
                "normal" => MemoryPressure::Normal,
                "warning" => MemoryPressure::Warning,
                "critical" => MemoryPressure::Critical,
                _ => return Err(format!("Unknown memory pressure: {}", raw.memory_pressure)),
            }
        };

        // Calculate energy impact if not provided
        let energy_impact = if raw.energy_impact > 0.0 {
            raw.energy_impact
        } else {
            raw.cpu_power_mw + raw.gpu_power_mw.unwrap_or(0.0)
        };

        Ok(MetricsEvent {
            timestamp: raw.timestamp,
            cpu_power_mw: raw.cpu_power_mw,
            cpu_usage_percent: raw.cpu_usage_percent,
            gpu_power_mw: raw.gpu_power_mw,
            gpu_usage_percent: raw.gpu_usage_percent,
            memory_pressure,
            memory_used_mb: raw.memory_used_mb,
            energy_impact,
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

/// Disk/filesystem event from system monitoring
///
/// Represents disk I/O activity and filesystem operations captured from
/// `iostat` and other macOS disk monitoring tools.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiskEvent {
    /// When the disk metrics were sampled
    pub timestamp: Timestamp,
    /// Disk read rate in KB/s
    pub read_kb_per_sec: f64,
    /// Disk write rate in KB/s
    pub write_kb_per_sec: f64,
    /// Disk read operations per second
    pub read_ops_per_sec: f64,
    /// Disk write operations per second
    pub write_ops_per_sec: f64,
    /// Disk name/identifier (e.g., "disk0", "disk1")
    pub disk_name: String,
    /// Filesystem path being monitored (if available)
    pub filesystem_path: Option<String>,
}

impl DiskEvent {
    /// Parse a disk event from iostat output line
    ///
    /// Expected iostat format:
    /// ```text
    /// disk0       1.23     4.56     0.12     0.34
    /// ```
    /// Fields: device, KB/t, tps, MB/s (read), MB/s (write)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The line format is invalid
    /// - Values cannot be parsed as numbers
    pub fn from_iostat_line(line: &str) -> Result<Self, String> {
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.len() < 5 {
            return Err(format!("Invalid iostat line format: {}", line));
        }

        let disk_name = parts[0].to_string();

        // Parse KB/t (kilobytes per transfer)
        let _kb_per_transfer: f64 = parts[1]
            .parse()
            .map_err(|e| format!("Failed to parse KB/t '{}': {}", parts[1], e))?;

        // Parse tps (transfers per second)
        let transfers_per_sec: f64 = parts[2]
            .parse()
            .map_err(|e| format!("Failed to parse tps '{}': {}", parts[2], e))?;

        // Parse MB/s read
        let mb_read_per_sec: f64 = parts[3]
            .parse()
            .map_err(|e| format!("Failed to parse read MB/s '{}': {}", parts[3], e))?;

        // Parse MB/s write
        let mb_write_per_sec: f64 = parts[4]
            .parse()
            .map_err(|e| format!("Failed to parse write MB/s '{}': {}", parts[4], e))?;

        // Convert MB/s to KB/s
        let read_kb_per_sec = mb_read_per_sec * 1024.0;
        let write_kb_per_sec = mb_write_per_sec * 1024.0;

        // Estimate operations per second from transfers and KB/t
        // This is an approximation since iostat doesn't separate read/write ops
        let total_ops_per_sec = transfers_per_sec;
        let read_ratio = if read_kb_per_sec + write_kb_per_sec > 0.0 {
            read_kb_per_sec / (read_kb_per_sec + write_kb_per_sec)
        } else {
            0.5 // Default to 50/50 split if no I/O
        };

        let read_ops_per_sec = total_ops_per_sec * read_ratio;
        let write_ops_per_sec = total_ops_per_sec * (1.0 - read_ratio);

        Ok(DiskEvent {
            timestamp: Utc::now(),
            read_kb_per_sec,
            write_kb_per_sec,
            read_ops_per_sec,
            write_ops_per_sec,
            disk_name,
            filesystem_path: None,
        })
    }

    /// Parse a disk event from JSON format (for testing/alternative sources)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JSON is malformed
    /// - Required fields are missing
    pub fn from_json(json: &str) -> Result<Self, String> {
        #[derive(Debug, Deserialize)]
        struct RawDiskEntry {
            #[serde(default = "chrono::Utc::now")]
            timestamp: Timestamp,
            read_kb_per_sec: f64,
            write_kb_per_sec: f64,
            #[serde(default)]
            read_ops_per_sec: f64,
            #[serde(default)]
            write_ops_per_sec: f64,
            disk_name: String,
            filesystem_path: Option<String>,
        }

        let raw: RawDiskEntry =
            serde_json::from_str(json).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        Ok(DiskEvent {
            timestamp: raw.timestamp,
            read_kb_per_sec: raw.read_kb_per_sec,
            write_kb_per_sec: raw.write_kb_per_sec,
            read_ops_per_sec: raw.read_ops_per_sec,
            write_ops_per_sec: raw.write_ops_per_sec,
            disk_name: raw.disk_name,
            filesystem_path: raw.filesystem_path,
        })
    }
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
            cpu_power_mw: 1234.5,
            cpu_usage_percent: 75.0,
            gpu_power_mw: Some(567.8),
            gpu_usage_percent: Some(45.0),
            memory_pressure: MemoryPressure::Warning,
            memory_used_mb: 8192.0,
            energy_impact: 1802.3,
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: MetricsEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_metrics_event_from_plist() {
        let plist_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>processor</key>
    <dict>
        <key>cpu_power</key>
        <real>1234.5</real>
    </dict>
    <key>gpu</key>
    <dict>
        <key>gpu_power</key>
        <real>567.8</real>
    </dict>
</dict>
</plist>"#;

        let event = MetricsEvent::from_plist(plist_xml.as_bytes()).unwrap();
        assert_eq!(event.cpu_power_mw, 1234.5);
        assert_eq!(event.gpu_power_mw, Some(567.8));
        assert_eq!(event.memory_pressure, MemoryPressure::Normal);
        assert!(event.cpu_usage_percent > 0.0); // Should be estimated from power
        assert!(event.energy_impact > 0.0); // Should be calculated
    }

    #[test]
    fn test_metrics_event_from_plist_no_gpu() {
        let plist_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>processor</key>
    <dict>
        <key>cpu_power</key>
        <real>2000.0</real>
    </dict>
</dict>
</plist>"#;

        let event = MetricsEvent::from_plist(plist_xml.as_bytes()).unwrap();
        assert_eq!(event.cpu_power_mw, 2000.0);
        assert_eq!(event.gpu_power_mw, None);
        assert!(event.cpu_usage_percent > 0.0);
    }

    #[test]
    fn test_metrics_event_from_plist_with_memory_pressure() {
        let plist_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>processor</key>
    <dict>
        <key>cpu_power</key>
        <real>1500.0</real>
    </dict>
    <key>gpu</key>
    <dict>
        <key>gpu_power</key>
        <real>800.0</real>
    </dict>
    <key>memory</key>
    <dict>
        <key>memory_pressure</key>
        <string>Warning</string>
    </dict>
</dict>
</plist>"#;

        let event = MetricsEvent::from_plist(plist_xml.as_bytes()).unwrap();
        assert_eq!(event.cpu_power_mw, 1500.0);
        assert_eq!(event.gpu_power_mw, Some(800.0));
        assert_eq!(event.memory_pressure, MemoryPressure::Warning);
        assert_eq!(event.energy_impact, 2300.0); // 1500 + 800
    }

    #[test]
    fn test_metrics_event_from_plist_with_free_memory() {
        let plist_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>processor</key>
    <dict>
        <key>cpu_power</key>
        <real>1200.0</real>
    </dict>
    <key>memory</key>
    <dict>
        <key>free_memory_mb</key>
        <real>300.0</real>
    </dict>
</dict>
</plist>"#;

        let event = MetricsEvent::from_plist(plist_xml.as_bytes()).unwrap();
        assert_eq!(event.cpu_power_mw, 1200.0);
        assert_eq!(event.gpu_power_mw, None);
        assert_eq!(event.memory_pressure, MemoryPressure::Critical); // < 500MB = Critical
        assert_eq!(event.energy_impact, 1200.0); // Only CPU power
    }

    #[test]
    fn test_metrics_event_from_json() {
        let json = r#"{
            "timestamp": "2024-12-09T18:30:45.123456Z",
            "cpu_power_mw": 1234.5,
            "cpu_usage_percent": 80.0,
            "gpu_power_mw": 567.8,
            "gpu_usage_percent": 60.0,
            "memory_pressure": "Warning",
            "memory_used_mb": 4096.0,
            "energy_impact": 1802.3
        }"#;

        let event = MetricsEvent::from_json(json).unwrap();
        assert_eq!(event.cpu_power_mw, 1234.5);
        assert_eq!(event.cpu_usage_percent, 80.0);
        assert_eq!(event.gpu_power_mw, Some(567.8));
        assert_eq!(event.gpu_usage_percent, Some(60.0));
        assert_eq!(event.memory_pressure, MemoryPressure::Warning);
        assert_eq!(event.memory_used_mb, 4096.0);
        assert_eq!(event.energy_impact, 1802.3);
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

    #[test]
    fn test_disk_event_serialization() {
        let event = DiskEvent {
            timestamp: Utc::now(),
            read_kb_per_sec: 1024.0,
            write_kb_per_sec: 512.0,
            read_ops_per_sec: 10.0,
            write_ops_per_sec: 5.0,
            disk_name: "disk0".to_string(),
            filesystem_path: Some("/".to_string()),
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: DiskEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_disk_event_from_iostat_line() {
        let iostat_line = "disk0       4.00     2.50     1.50     0.75";
        let event = DiskEvent::from_iostat_line(iostat_line).unwrap();

        assert_eq!(event.disk_name, "disk0");
        assert_eq!(event.read_kb_per_sec, 1536.0); // 1.50 MB/s * 1024
        assert_eq!(event.write_kb_per_sec, 768.0); // 0.75 MB/s * 1024
        assert!(event.read_ops_per_sec > 0.0);
        assert!(event.write_ops_per_sec > 0.0);
        assert_eq!(event.filesystem_path, None);
    }

    #[test]
    fn test_disk_event_from_json() {
        let json = r#"{
            "timestamp": "2024-12-09T18:30:45.123456Z",
            "read_kb_per_sec": 1024.0,
            "write_kb_per_sec": 512.0,
            "read_ops_per_sec": 10.0,
            "write_ops_per_sec": 5.0,
            "disk_name": "disk0",
            "filesystem_path": "/"
        }"#;

        let event = DiskEvent::from_json(json).unwrap();
        assert_eq!(event.read_kb_per_sec, 1024.0);
        assert_eq!(event.write_kb_per_sec, 512.0);
        assert_eq!(event.read_ops_per_sec, 10.0);
        assert_eq!(event.write_ops_per_sec, 5.0);
        assert_eq!(event.disk_name, "disk0");
        assert_eq!(event.filesystem_path, Some("/".to_string()));
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
        cpu_power_mw: f64,
        gpu_power_mw: Option<f64>,
        memory_pressure: MemoryPressure,
    }

    impl Arbitrary for ValidMetricsData {
        fn arbitrary(g: &mut Gen) -> Self {
            // Generate valid CPU power (0-10000 mW is reasonable for a laptop)
            let cpu_power_mw = (u16::arbitrary(g) % 10001) as f64;

            // Generate optional GPU power (0-50000 mW is reasonable)
            let gpu_power_mw = if bool::arbitrary(g) {
                Some((u16::arbitrary(g) % 50001) as f64)
            } else {
                None
            };

            ValidMetricsData {
                cpu_power_mw,
                gpu_power_mw,
                memory_pressure: MemoryPressure::arbitrary(g),
            }
        }
    }

    impl ValidMetricsData {
        /// Convert to plist XML string in the format produced by powermetrics
        fn to_plist(&self) -> String {
            let gpu_section = if let Some(gpu_power) = self.gpu_power_mw {
                format!(
                    r#"    <key>gpu</key>
    <dict>
        <key>gpu_power</key>
        <real>{}</real>
    </dict>"#,
                    gpu_power
                )
            } else {
                String::new()
            };

            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>processor</key>
    <dict>
        <key>cpu_power</key>
        <real>{}</real>
    </dict>
{}
</dict>
</plist>"#,
                self.cpu_power_mw, gpu_section
            )
        }

        /// Convert to JSON string for testing alternative parsing
        fn to_json(&self) -> String {
            let memory_pressure_str = match self.memory_pressure {
                MemoryPressure::Normal => "Normal",
                MemoryPressure::Warning => "Warning",
                MemoryPressure::Critical => "Critical",
            };

            format!(
                r#"{{
                    "timestamp": "2024-12-09T18:30:45.123456Z",
                    "cpu_power_mw": {},
                    "gpu_power_mw": {},
                    "memory_pressure": "{}"
                }}"#,
                self.cpu_power_mw,
                match self.gpu_power_mw {
                    Some(val) => val.to_string(),
                    None => "null".to_string(),
                },
                memory_pressure_str
            )
        }
    }

    // Feature: macos-system-observer, Property 4: Metrics parsing extracts all fields
    // Validates: Requirements 2.2
    #[quickcheck]
    fn prop_metrics_parsing_extracts_all_fields_plist(data: ValidMetricsData) -> bool {
        // Generate plist in the format produced by powermetrics
        let plist = data.to_plist();

        // Parse the plist
        let parsed = match MetricsEvent::from_plist(plist.as_bytes()) {
            Ok(event) => event,
            Err(_) => return false,
        };

        // Verify all fields are extracted correctly
        (parsed.cpu_power_mw - data.cpu_power_mw).abs() < 0.001
            && parsed.gpu_power_mw == data.gpu_power_mw
    }

    // Feature: macos-system-observer, Property 4b: JSON metrics parsing for testing
    // Validates: Requirements 2.2 (alternative format)
    #[quickcheck]
    fn prop_metrics_parsing_extracts_all_fields_json(data: ValidMetricsData) -> bool {
        // Generate JSON for testing
        let json = data.to_json();

        // Parse the JSON
        let parsed = match MetricsEvent::from_json(&json) {
            Ok(event) => event,
            Err(_) => return false,
        };

        // Verify all fields are extracted correctly
        (parsed.cpu_power_mw - data.cpu_power_mw).abs() < 0.001
            && parsed.gpu_power_mw == data.gpu_power_mw
            && parsed.memory_pressure == data.memory_pressure
    }
}
