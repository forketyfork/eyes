/// AI analyzer and backend implementations
pub mod analyzer;
pub mod backends;

pub use analyzer::{AIAnalyzer, AIInsight};
pub use backends::{LLMBackend, MockBackend, OllamaBackend, OpenAIBackend};

// Re-export Severity from events module for consistency
pub use crate::events::Severity;
