/// Error types for the system observer
pub mod error;

/// Core event types and data structures
pub mod events;

/// Data collectors for logs and metrics
pub mod collectors;

/// Event aggregator with rolling buffer
pub mod aggregator;

/// Trigger logic for AI analysis activation
pub mod triggers;

/// AI analyzer and backend implementations
pub mod ai;

/// Alert manager and notifications
pub mod alerts;

/// Configuration management
pub mod config;

/// Self-monitoring and metrics collection
pub mod monitoring;

/// Web dashboard and alert history API
pub mod web;

/// MCP server for agent-driven alert triage
pub mod mcp;

// Re-export commonly used types
pub use error::{AlertError, AnalysisError, CollectorError, ConfigError};
pub use events::{
    DiskEvent, LogEvent, MeasurementKind, MemoryPressure, MessageType, MetricsEvent,
    MetricsProvenance, MetricsSource, ProcessMetric, Severity, Timestamp,
};
