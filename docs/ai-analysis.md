# AI Analysis System

Eyes uses artificial intelligence to analyze macOS system events and provide actionable insights. The AI analysis system coordinates between trigger events, prompt generation, and LLM backends to deliver intelligent diagnostics.

## Overview

The AI analysis pipeline consists of:

1. **Trigger Context**: Recent system events that triggered analysis
2. **Prompt Generation**: Structured prompts with system context
3. **LLM Backend**: AI model that performs the analysis
4. **Insight Generation**: Structured results with recommendations
5. **Notification Delivery**: User-facing alerts and suggestions

## Core Components

### AIAnalyzer

The central coordinator that orchestrates AI analysis:

```rust
use eyes::ai::AIAnalyzer;
use eyes::ai::backends::OllamaBackend;

// Create analyzer with Ollama backend
let backend = OllamaBackend::new(
    "http://localhost:11434".to_string(),
    "llama3".to_string()
);
let analyzer = AIAnalyzer::with_backend(Arc::new(backend));

// Analyze trigger context
let insight = analyzer.analyze(&context).await?;
```

**Key Methods:**
- `analyze()`: Perform AI analysis on trigger context
- `summarize_activity()`: Generate periodic system summaries
- `format_prompt()`: Create structured prompts for LLM backends

### AIInsight

Structured analysis results with actionable information:

```rust
pub struct AIInsight {
    pub timestamp: Timestamp,
    pub summary: String,
    pub root_cause: Option<String>,
    pub recommendations: Vec<String>,
    pub severity: Severity,
}
```

**Features:**
- **Timestamp**: When the analysis was performed
- **Summary**: Brief description of the main issue
- **Root Cause**: Most likely underlying cause (optional)
- **Recommendations**: Specific actionable steps
- **Severity**: Info, Warning, or Critical classification

**Utility Methods:**
- `is_critical()`: Check if immediate attention required
- `notification_title()`: Get title for macOS notifications
- `notification_body()`: Get body text for notifications

## Prompt Engineering

### Context Analysis

The AI analyzer creates comprehensive prompts that include:

**System Metrics:**
- Time window duration and event counts
- Average CPU usage and power consumption
- GPU usage and power consumption (when available)
- Memory usage and pressure levels
- Energy impact measurements

**Error Analysis:**
- Recent error and fault messages with timestamps
- Process and subsystem information
- Message content and severity levels
- Error frequency and patterns

**Resource Trends:**
- CPU and GPU power consumption over time
- Memory pressure evolution
- Energy impact changes
- System performance indicators

### Prompt Structure

Generated prompts follow a consistent structure:

```
You are a macOS system diagnostics expert. Analyze the following system data and provide:
1. A concise summary of the issue
2. The likely root cause
3. Actionable recommendations

System Context:
- Time Window: 60 seconds
- Error Count: 3
- Fault Count: 1
- Total Log Events: 15
- Total Metrics Events: 12
- Memory Pressure: Warning
- Average CPU Usage: 75.2%
- Average CPU Power: 3500.0mW
- Average GPU Usage: 45.1%
- Average GPU Power: 2100.0mW
- Average Memory Used: 8192.0MB
- Energy Impact: 5600.0mW
- Triggered By: ErrorFrequencyRule
- Trigger Reason: Rule 'ErrorFrequencyRule' triggered

Recent Errors:
[10:30:45] com.apple.Safari/Safari: Error - Failed to load resource
[10:30:47] com.apple.WindowServer/WindowServer: Fault - GPU memory allocation failed
[10:30:50] com.apple.Safari/Safari: Error - JavaScript execution timeout

Recent Metrics:
[10:30:40] CPU: 70.0% (3000.0mW), GPU: 40.0% (2000.0mW), Memory: 7680.0MB (Warning), Energy: 5000.0mW
[10:30:45] CPU: 80.0% (4000.0mW), GPU: 50.0% (2200.0mW), Memory: 8192.0MB (Warning), Energy: 6200.0mW
[10:30:50] CPU: 75.0% (3500.0mW), GPU: 45.0% (2100.0mW), Memory: 8704.0MB (Critical), Energy: 5600.0mW

Respond in JSON format with fields:
- summary (string): Brief description of the main issue
- root_cause (string or null): Most likely underlying cause
- recommendations (array of strings): Specific actionable steps
- severity (string): "info", "warning", or "critical"
```

### Response Processing

The system handles various LLM response formats with advanced extraction capabilities:

**Expected JSON Format:**
```json
{
  "summary": "High memory usage in Safari with GPU allocation failures",
  "root_cause": "Multiple browser tabs with heavy JavaScript and WebGL content",
  "recommendations": [
    "Close unused browser tabs to free memory",
    "Restart Safari to clear GPU memory leaks",
    "Check Activity Monitor for memory-intensive processes"
  ],
  "severity": "warning"
}
```

**Robust Parsing:**
- Extracts JSON from markdown code blocks (```json ... ```)
- Handles mixed content with explanatory text surrounding JSON
- Searches for JSON boundaries using { and } markers
- Graceful error handling for malformed responses
- Fallback parsing for partial responses
- Property-based testing ensures extraction works across various response formats

## Analysis Workflows

### Triggered Analysis

Initiated when trigger rules detect system issues:

1. **Rule Evaluation**: Trigger engine evaluates recent events
2. **Context Creation**: Package relevant events into TriggerContext
3. **Prompt Generation**: Format structured prompt with system data
4. **LLM Analysis**: Send prompt to configured backend
5. **Response Processing**: Parse and validate LLM response
6. **Insight Creation**: Generate AIInsight with recommendations
7. **Notification**: Deliver alerts based on severity

### Periodic Summaries

Regular system health assessments:

1. **Event Collection**: Gather recent logs and metrics
2. **Summary Context**: Create context for general analysis
3. **Health Assessment**: Analyze overall system state
4. **Trend Analysis**: Identify patterns and potential issues
5. **Preventive Recommendations**: Suggest maintenance actions

### On-Demand Analysis

User-initiated analysis for specific issues:

1. **Custom Context**: Focus on specific time ranges or events
2. **Targeted Analysis**: Deep dive into particular subsystems
3. **Comparative Analysis**: Compare current vs. historical data
4. **Detailed Recommendations**: Comprehensive action plans

## Error Handling

### Backend Failures

Robust handling of AI backend issues:

- **Connection Errors**: Network timeouts and connectivity issues
- **Authentication Failures**: Invalid API keys or permissions
- **Rate Limiting**: API quota exceeded or throttling
- **Service Unavailable**: Backend maintenance or outages
- **Invalid Responses**: Malformed or incomplete responses

### Graceful Degradation

When AI analysis fails:

- **Fallback Insights**: Generate basic insights from trigger rules
- **Error Logging**: Record failures for debugging
- **User Notification**: Inform users of reduced functionality
- **Retry Logic**: Attempt recovery with exponential backoff

### Response Validation

Ensure analysis quality:

- **Schema Validation**: Verify JSON structure and required fields
- **Content Validation**: Check for reasonable recommendations
- **Severity Validation**: Ensure appropriate severity levels
- **Length Limits**: Prevent excessively long responses

## Performance Optimization

### Prompt Efficiency

- **Structured Data**: Use consistent formatting to reduce token usage
- **Relevant Context**: Include only pertinent information
- **Token Limits**: Cap prompts at reasonable lengths
- **Template Reuse**: Standardize prompt structures

### Response Caching

- **Context Similarity**: Cache responses for similar system states
- **Time-based Expiry**: Invalidate cached responses after time periods
- **Event Fingerprinting**: Identify similar event patterns
- **Memory Management**: Limit cache size and cleanup old entries

### Async Processing

- **Non-blocking Analysis**: Don't block system monitoring during AI analysis
- **Background Processing**: Queue analysis requests for processing
- **Timeout Handling**: Prevent hanging on slow backends
- **Concurrent Requests**: Handle multiple analysis requests efficiently

## MockBackend for Testing

The `MockBackend` provides configurable responses for testing and development:

```rust
use eyes::ai::backends::MockBackend;

// Success response
let backend = MockBackend::success();

// Error response
let backend = MockBackend::error("Simulated failure".to_string());

// Timeout simulation
let backend = MockBackend::timeout();

// Multiple responses with cycling
let responses = vec![
    Ok(success_insight),
    Err(AnalysisError::BackendError("Temporary error".to_string()))
];
let backend = MockBackend::with_responses(responses);

// Add delay for timeout testing
let backend = MockBackend::success().with_delay(Duration::from_secs(5));
```

**Features:**
- Configurable success/error responses
- Multiple response cycling
- Call count and context tracking
- Delay simulation for timeout testing
- Reset functionality for test isolation

## Testing Strategy

### Unit Tests

- **Prompt Generation**: Verify correct prompt formatting
- **Response Parsing**: Test JSON extraction and validation
- **Error Handling**: Ensure graceful failure handling
- **Insight Creation**: Validate AIInsight construction

### Property-Based Tests

- **Prompt Consistency**: Ensure prompts contain required sections
- **Response Robustness**: Handle various LLM response formats
- **Error Resilience**: Graceful handling of malformed data
- **Backend Integration**: Verify correct backend communication
- **Failure Recovery**: Test retry behavior and error handling

### Integration Tests

- **End-to-End**: Full analysis pipeline with real backends
- **Mock Scenarios**: Comprehensive testing with MockBackend
- **Failure Simulation**: Test various failure conditions
- **Performance**: Measure analysis latency and resource usage

## Configuration

### Backend Selection

```toml
[ai]
# Local inference (recommended)
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"

# Cloud inference
# backend = "openai"
# api_key = "sk-..."
# model = "gpt-4"
```

### Analysis Parameters

```toml
[ai.analysis]
# Request timeout (seconds)
timeout = 60

# Maximum prompt length (tokens)
max_prompt_tokens = 2000

# Maximum response length (tokens)
max_response_tokens = 1000

# Temperature for analysis (0.0-1.0)
temperature = 0.1

# Enable response caching
enable_caching = true

# Cache expiry time (seconds)
cache_expiry = 300
```

### Quality Controls

```toml
[ai.quality]
# Minimum confidence threshold
min_confidence = 0.7

# Maximum recommendations per insight
max_recommendations = 5

# Enable response validation
validate_responses = true

# Fallback to rule-based insights on AI failure
enable_fallback = true
```

## Monitoring and Debugging

### Analysis Metrics

Track AI system performance:

- **Request Latency**: Time from trigger to insight
- **Success Rate**: Percentage of successful analyses
- **Backend Health**: Availability and response times
- **Cache Hit Rate**: Effectiveness of response caching

### Debug Logging

Enable detailed logging for troubleshooting:

```bash
# Enable AI analysis debugging (via environment variable)
RUST_LOG=eyes::ai=debug cargo run

# Enable AI analysis debugging (via CLI flag)
cargo run -- --verbose

# View prompt generation (environment variable only)
RUST_LOG=eyes::ai::analyzer=trace cargo run

# Monitor backend communication (environment variable only)
RUST_LOG=eyes::ai::backends=debug cargo run
```

### Error Analysis

Common issues and solutions:

- **Empty Insights**: Check prompt generation and backend connectivity
- **Poor Recommendations**: Verify model selection and prompt engineering
- **High Latency**: Monitor backend performance and network connectivity
- **Parsing Failures**: Review LLM response formats and extraction logic

## Best Practices

### Prompt Design

- **Clear Instructions**: Specify exactly what analysis is needed
- **Structured Data**: Use consistent formatting for system metrics
- **Context Relevance**: Include only pertinent information
- **Response Format**: Clearly specify expected JSON structure

### Backend Management

- **Health Monitoring**: Regularly check backend availability
- **Fallback Planning**: Have alternative backends configured
- **Rate Limiting**: Respect API limits and implement backoff
- **Security**: Protect API keys and use secure connections

### Quality Assurance

- **Response Validation**: Always validate LLM responses
- **Human Review**: Periodically review AI recommendations
- **Feedback Loop**: Use user feedback to improve prompts
- **Continuous Testing**: Regularly test with various scenarios