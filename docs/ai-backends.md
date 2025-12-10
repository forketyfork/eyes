# AI Backends

Eyes supports multiple AI backends for system diagnostics. Choose based on your privacy requirements and performance needs.

## Ollama (Recommended)

Local LLM execution for complete privacy—your system data never leaves your machine.

### Setup

```bash
# Install Ollama
brew install ollama

# Pull a model
ollama pull llama3

# Start Ollama service
ollama serve
```

### Configuration

```toml
[ai]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"
```

### Recommended Models

- **llama3** (8B): Fast, good balance of speed and quality
- **llama3:70b**: Higher quality analysis, slower
- **mistral**: Alternative with good performance
- **codellama**: Optimized for technical diagnostics

### Performance

- First analysis may be slow (model loading)
- Subsequent analyses are faster (model stays in memory)
- Requires ~8GB RAM for 8B models, ~40GB for 70B models

## OpenAI

Cloud-based alternative for enhanced capabilities when privacy is less critical.

### Setup

1. Get an API key from [platform.openai.com](https://platform.openai.com)
2. Configure Eyes with your key

### Configuration

```toml
[ai]
backend = "openai"
api_key = "sk-..."
model = "gpt-4"
```

### Recommended Models

- **gpt-4**: Highest quality analysis
- **gpt-4-turbo**: Faster, lower cost
- **gpt-3.5-turbo**: Budget option, still effective

### Privacy Considerations

- System logs and metrics are sent to OpenAI servers
- Data may contain sensitive information (process names, error messages)
- Subject to OpenAI's data usage policies
- Requires internet connection

## Prompt Format

Both backends receive the same structured prompt:

```
You are a macOS system diagnostics expert. Analyze the following system data and provide:
1. A concise summary of the issue
2. The likely root cause
3. Actionable recommendations

System Context:
- Time Window: {duration}
- Error Count: {count}
- Memory Pressure: {pressure}

Recent Errors:
{log_entries}

Recent Metrics:
{metrics}

Respond in JSON format with fields: summary, root_cause, recommendations (array), severity (info/warning/critical).
```

## Response Format

Expected JSON response from AI:

```json
{
  "summary": "High memory pressure detected",
  "root_cause": "Safari consuming 8GB RAM with 50+ tabs open",
  "recommendations": [
    "Close unused Safari tabs",
    "Restart Safari to clear memory leaks",
    "Consider using tab suspender extension"
  ],
  "severity": "warning"
}
```

## Error Handling

- **Timeout**: 30 second timeout per request
- **Connection failures**: Logged and queued for retry
- **Invalid responses**: Logged, notification skipped
- **Rate limits**: Exponential backoff retry

## AI Analyzer Integration

The `AIAnalyzer` coordinates with backends through the `LLMBackend` trait:

```rust
use crate::ai::{AIAnalyzer, AIInsight};
use std::sync::Arc;

// Create analyzer with Ollama backend
let backend = Arc::new(OllamaBackend::new("http://localhost:11434", "llama3"));
let analyzer = AIAnalyzer::with_backend(backend);

// Analyze trigger context
let insight = analyzer.analyze(&trigger_context).await?;

// Generate activity summary
let summary = analyzer.summarize_activity(&log_events, &metrics_events).await?;
```

## AIInsight Structure

Analysis results are returned as structured insights:

```rust
pub struct AIInsight {
    pub timestamp: Timestamp,
    pub severity: Severity,           // Info, Warning, Critical
    pub title: String,               // Brief summary
    pub description: String,         // Detailed analysis
    pub recommendations: Vec<String>, // Actionable steps
    pub confidence: f64,             // 0.0 to 1.0
    pub tags: Vec<String>,           // Categorization
}
```

## Testing

The analyzer includes a placeholder backend for testing:

```rust
// Creates analyzer with non-functional placeholder
let analyzer = AIAnalyzer::new();

// For tests, use mock backends
let backend = Arc::new(MockBackend::new());
let analyzer = AIAnalyzer::with_backend(backend);
```
