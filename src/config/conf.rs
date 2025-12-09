/// Placeholder for Config implementation
pub struct Config {
    // Implementation will be added in later tasks
}

/// Placeholder for AIBackendConfig
pub enum AIBackendConfig {
    Ollama { endpoint: String, model: String },
    OpenAI { api_key: String, model: String },
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Config {
    pub fn new() -> Self {
        Self {}
    }
}
