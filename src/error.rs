use thiserror::Error;

/// Errors that can occur in data collectors
#[derive(Error, Debug)]
pub enum CollectorError {
    #[error("Failed to spawn subprocess: {0}")]
    SubprocessSpawn(String),

    #[error("Subprocess terminated unexpectedly: {0}")]
    SubprocessTerminated(String),

    #[error("Failed to parse output: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Errors that can occur during AI analysis
#[derive(Error, Debug)]
pub enum AnalysisError {
    #[error("Backend communication failed: {0}")]
    BackendError(String),

    #[error("Request timeout")]
    Timeout,

    #[error("Invalid response format: {0}")]
    InvalidResponse(String),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),
}

/// Errors that can occur when sending alerts
#[derive(Error, Debug)]
pub enum AlertError {
    #[error("Failed to send notification: {0}")]
    NotificationFailed(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Errors that can occur during configuration loading
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(String),

    #[error("Failed to parse config: {0}")]
    ParseError(String),

    #[error("Invalid configuration value: {0}")]
    ValidationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    TomlError(#[from] toml::de::Error),
}
