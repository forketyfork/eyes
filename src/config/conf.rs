use crate::error::ConfigError;
use crate::events::MemoryPressure;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// Main configuration structure for the macOS System Observer
///
/// This structure contains all configurable parameters for the application,
/// including log filtering, metrics collection, buffer management, trigger
/// thresholds, AI backend selection, and alert settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Predicate filter for log stream (Apple's query language)
    #[serde(default = "default_log_predicate")]
    pub log_predicate: String,

    /// Interval between metrics samples (in seconds)
    #[serde(default = "default_metrics_interval_secs")]
    pub metrics_interval_secs: u64,

    /// Maximum age of events in the rolling buffer (in seconds)
    #[serde(default = "default_buffer_max_age_secs")]
    pub buffer_max_age_secs: u64,

    /// Maximum number of events in the rolling buffer
    #[serde(default = "default_buffer_max_size")]
    pub buffer_max_size: usize,

    /// Number of errors required to trigger AI analysis
    #[serde(default = "default_error_threshold")]
    pub error_threshold: usize,

    /// Time window for error counting (in seconds)
    #[serde(default = "default_error_window_secs")]
    pub error_window_secs: u64,

    /// Memory pressure level that triggers AI analysis
    #[serde(default = "default_memory_threshold")]
    pub memory_threshold: MemoryPressure,

    /// AI backend configuration
    #[serde(default)]
    pub ai_backend: AIBackendConfig,

    /// Maximum number of alerts per minute
    #[serde(default = "default_alert_rate_limit")]
    pub alert_rate_limit: usize,
}

/// AI backend configuration options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum AIBackendConfig {
    /// Local Ollama instance
    Ollama {
        /// Ollama API endpoint URL
        #[serde(default = "default_ollama_endpoint")]
        endpoint: String,
        /// Model name to use
        #[serde(default = "default_ollama_model")]
        model: String,
    },
    /// OpenAI cloud API
    OpenAI {
        /// OpenAI API key
        api_key: String,
        /// Model name to use
        #[serde(default = "default_openai_model")]
        model: String,
    },
}

// Default value functions for serde
fn default_log_predicate() -> String {
    "messageType == error OR messageType == fault".to_string()
}

fn default_metrics_interval_secs() -> u64 {
    5
}

fn default_buffer_max_age_secs() -> u64 {
    60
}

fn default_buffer_max_size() -> usize {
    1000
}

fn default_error_threshold() -> usize {
    5
}

fn default_error_window_secs() -> u64 {
    10
}

fn default_memory_threshold() -> MemoryPressure {
    MemoryPressure::Warning
}

fn default_alert_rate_limit() -> usize {
    3
}

fn default_ollama_endpoint() -> String {
    "http://localhost:11434".to_string()
}

fn default_ollama_model() -> String {
    "llama3".to_string()
}

fn default_openai_model() -> String {
    "gpt-4".to_string()
}

impl Default for AIBackendConfig {
    fn default() -> Self {
        AIBackendConfig::Ollama {
            endpoint: default_ollama_endpoint(),
            model: default_ollama_model(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_predicate: default_log_predicate(),
            metrics_interval_secs: default_metrics_interval_secs(),
            buffer_max_age_secs: default_buffer_max_age_secs(),
            buffer_max_size: default_buffer_max_size(),
            error_threshold: default_error_threshold(),
            error_window_secs: default_error_window_secs(),
            memory_threshold: default_memory_threshold(),
            ai_backend: AIBackendConfig::default(),
            alert_rate_limit: default_alert_rate_limit(),
        }
    }
}

impl Config {
    /// Create a new Config with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration from a TOML file
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the TOML configuration file
    ///
    /// # Errors
    ///
    /// Returns a `ConfigError` if:
    /// - The file cannot be read
    /// - The TOML is malformed
    /// - Configuration values fail validation
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use eyes::config::Config;
    /// use std::path::Path;
    ///
    /// let config = Config::from_file(Path::new("config.toml")).unwrap();
    /// ```
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        // Read the file contents
        let contents = std::fs::read_to_string(path).map_err(|e| {
            ConfigError::ReadError(format!("Failed to read config file '{}': {}", path.display(), e))
        })?;

        // Parse TOML
        let config: Config = toml::from_str(&contents)?;

        // Validate the configuration
        config.validate()?;

        Ok(config)
    }

    /// Validate configuration values
    ///
    /// Ensures all configuration values are within acceptable ranges and
    /// logically consistent.
    ///
    /// # Errors
    ///
    /// Returns a `ConfigError::ValidationError` if any values are invalid.
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate metrics interval (must be at least 1 second)
        if self.metrics_interval_secs == 0 {
            return Err(ConfigError::ValidationError(
                "metrics_interval_secs must be at least 1".to_string(),
            ));
        }

        // Validate buffer max age (must be at least 1 second)
        if self.buffer_max_age_secs == 0 {
            return Err(ConfigError::ValidationError(
                "buffer_max_age_secs must be at least 1".to_string(),
            ));
        }

        // Validate buffer max size (must be at least 1)
        if self.buffer_max_size == 0 {
            return Err(ConfigError::ValidationError(
                "buffer_max_size must be at least 1".to_string(),
            ));
        }

        // Validate error threshold (must be at least 1)
        if self.error_threshold == 0 {
            return Err(ConfigError::ValidationError(
                "error_threshold must be at least 1".to_string(),
            ));
        }

        // Validate error window (must be at least 1 second)
        if self.error_window_secs == 0 {
            return Err(ConfigError::ValidationError(
                "error_window_secs must be at least 1".to_string(),
            ));
        }

        // Validate alert rate limit (must be at least 1)
        if self.alert_rate_limit == 0 {
            return Err(ConfigError::ValidationError(
                "alert_rate_limit must be at least 1".to_string(),
            ));
        }

        // Validate AI backend configuration
        match &self.ai_backend {
            AIBackendConfig::Ollama { endpoint, model } => {
                if endpoint.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "Ollama endpoint cannot be empty".to_string(),
                    ));
                }
                if model.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "Ollama model cannot be empty".to_string(),
                    ));
                }
            }
            AIBackendConfig::OpenAI { api_key, model } => {
                if api_key.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "OpenAI API key cannot be empty".to_string(),
                    ));
                }
                if model.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "OpenAI model cannot be empty".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Get metrics interval as a Duration
    pub fn metrics_interval(&self) -> Duration {
        Duration::from_secs(self.metrics_interval_secs)
    }

    /// Get buffer max age as a Duration
    pub fn buffer_max_age(&self) -> Duration {
        Duration::from_secs(self.buffer_max_age_secs)
    }

    /// Get error window as a Duration
    pub fn error_window(&self) -> Duration {
        Duration::from_secs(self.error_window_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.log_predicate, "messageType == error OR messageType == fault");
        assert_eq!(config.metrics_interval_secs, 5);
        assert_eq!(config.buffer_max_age_secs, 60);
        assert_eq!(config.buffer_max_size, 1000);
        assert_eq!(config.error_threshold, 5);
        assert_eq!(config.error_window_secs, 10);
        assert_eq!(config.memory_threshold, MemoryPressure::Warning);
        assert_eq!(config.alert_rate_limit, 3);
        
        // Validate default config
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_from_valid_toml() {
        let toml_content = r#"
            log_predicate = "messageType == error"
            metrics_interval_secs = 10
            buffer_max_age_secs = 120
            buffer_max_size = 2000
            error_threshold = 10
            error_window_secs = 20
            memory_threshold = "Critical"
            alert_rate_limit = 5

            [ai_backend]
            backend = "ollama"
            endpoint = "http://localhost:11434"
            model = "llama3"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();
        assert_eq!(config.log_predicate, "messageType == error");
        assert_eq!(config.metrics_interval_secs, 10);
        assert_eq!(config.buffer_max_age_secs, 120);
        assert_eq!(config.buffer_max_size, 2000);
        assert_eq!(config.error_threshold, 10);
        assert_eq!(config.error_window_secs, 20);
        assert_eq!(config.memory_threshold, MemoryPressure::Critical);
        assert_eq!(config.alert_rate_limit, 5);
    }

    #[test]
    fn test_config_with_openai_backend() {
        let toml_content = r#"
            [ai_backend]
            backend = "openai"
            api_key = "sk-test-key"
            model = "gpt-4"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();
        match config.ai_backend {
            AIBackendConfig::OpenAI { api_key, model } => {
                assert_eq!(api_key, "sk-test-key");
                assert_eq!(model, "gpt-4");
            }
            _ => panic!("Expected OpenAI backend"),
        }
    }

    #[test]
    fn test_config_with_partial_values_uses_defaults() {
        let toml_content = r#"
            error_threshold = 15
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();
        assert_eq!(config.error_threshold, 15);
        // Other values should be defaults
        assert_eq!(config.metrics_interval_secs, 5);
        assert_eq!(config.buffer_max_size, 1000);
    }

    #[test]
    fn test_config_validation_zero_metrics_interval() {
        let config = Config {
            metrics_interval_secs: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_buffer_age() {
        let config = Config {
            buffer_max_age_secs: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_buffer_size() {
        let config = Config {
            buffer_max_size: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_error_threshold() {
        let config = Config {
            error_threshold: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_error_window() {
        let config = Config {
            error_window_secs: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_alert_rate_limit() {
        let config = Config {
            alert_rate_limit: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_empty_ollama_endpoint() {
        let config = Config {
            ai_backend: AIBackendConfig::Ollama {
                endpoint: String::new(),
                model: "llama3".to_string(),
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_empty_ollama_model() {
        let config = Config {
            ai_backend: AIBackendConfig::Ollama {
                endpoint: "http://localhost:11434".to_string(),
                model: String::new(),
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_empty_openai_api_key() {
        let config = Config {
            ai_backend: AIBackendConfig::OpenAI {
                api_key: String::new(),
                model: "gpt-4".to_string(),
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_empty_openai_model() {
        let config = Config {
            ai_backend: AIBackendConfig::OpenAI {
                api_key: "sk-test".to_string(),
                model: String::new(),
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_duration_helpers() {
        let config = Config::default();
        assert_eq!(config.metrics_interval(), Duration::from_secs(5));
        assert_eq!(config.buffer_max_age(), Duration::from_secs(60));
        assert_eq!(config.error_window(), Duration::from_secs(10));
    }

    #[test]
    fn test_config_from_nonexistent_file() {
        let result = Config::from_file(Path::new("/nonexistent/config.toml"));
        assert!(result.is_err());
        match result {
            Err(ConfigError::ReadError(_)) => (),
            _ => panic!("Expected ReadError"),
        }
    }

    #[test]
    fn test_config_from_malformed_toml() {
        let toml_content = r#"
            this is not valid toml [[[
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let result = Config::from_file(temp_file.path());
        assert!(result.is_err());
        match result {
            Err(ConfigError::TomlError(_)) => (),
            _ => panic!("Expected TomlError"),
        }
    }
}
