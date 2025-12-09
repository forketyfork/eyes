/// Log stream collector for macOS Unified Log System
pub mod log_collector;

/// Metrics collector for system resource monitoring
pub mod metrics_collector;

pub use log_collector::LogCollector;
pub use metrics_collector::MetricsCollector;
