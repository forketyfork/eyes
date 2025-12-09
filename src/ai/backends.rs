use crate::ai::AIInsight;
use crate::error::AnalysisError;
use crate::triggers::TriggerContext;

/// Trait for LLM backend implementations
pub trait LLMBackend: Send {
    fn analyze(&self, context: &TriggerContext) -> Result<AIInsight, AnalysisError>;
}

// Placeholder for backend implementations
// Will be added in later tasks
