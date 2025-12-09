/// Error types for the system observer
pub mod error;

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

// Re-export commonly used types
pub use error::{AlertError, AnalysisError, CollectorError, ConfigError};
