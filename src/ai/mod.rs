/// AI analyzer and backend implementations
pub mod analyzer;
pub mod backends;

pub use analyzer::{AIAnalyzer, AIInsight, Severity};
pub use backends::LLMBackend;
