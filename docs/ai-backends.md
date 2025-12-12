# AI Backends

Eyes integrates with Large Language Models (LLMs) to provide intelligent analysis of macOS system events. The AI backend system supports both local and cloud-based inference with robust error handling and response parsing.

## Overview

The AI backend architecture consists of:

- **LLMBackend Trait**: Common interface for all AI backends
- **OllamaBackend**: Local inference using Ollama server
- **OpenAIBackend**: Cloud-based inference using OpenAI API
- **MockBackend**: Testing and development backend with configurable responses

## Backend Implementations

### Ollama Backend (Recommended)

Local LLM inference for complete privacy and offline operation.

**Features:**
- Privacy-first: All data stays on your machine
- No API costs or rate limits
- Works offline
- Optimized for Apple Silicon Macs
- Supports multiple open-source models (Llama 3, Mistral, etc.)

**Configuration:**
```toml
[ai]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"
```

**Setup:**
```bash
# Install Ollama
brew install ollama

# Pull a model
ollama pull llama3

# Start Ollama service
ollama serve
```

**Supported Models:**
- `llama3` - Meta's Llama 3 (recommended for system analysis)
- `mistral` - Mistral 7B (faster, good for basic analysis)
- `codellama` - Code-focused variant of Llama
- `phi3` - Microsoft's Phi-3 (compact, efficient)

### OpenAI Backend

Cloud-based inference using OpenAI's GPT models.

**Features:**
- State-of-the-art analysis quality
- Structured JSON response format
- Fast inference times
- Requires internet connection and API key

**Configuration:**
```toml
[ai]
backend = "openai"
api_key = "sk-..."
model = "gpt-4"
base_url = "https://api.openai.com/v1"  # Optional, for custom endpoints
```

**Supported Models:**
- `gpt-4` - Best analysis quality (recommended)
- `gpt-3.5-turbo` - Faster and cheaper alternative
- Custom models via compatible APIs

### Mock Backend

Development and testing backend with configurable responses.

**Features:**
- Deterministic responses for testing
- Configurable delays and failures
- Call tracking and context inspection
- No external dependencies

**Usage:**
```rust
use eyes::ai::backends::MockBackend;
use eyes::ai::AIInsight;
use eyes::events::Severity;

// Success response
let backend = MockBackend::success();

// Custom response
let insight = AIInsight::new(
    "Custom analysis".to_string(),
    Some("Root cause".to_string()),
    vec!["Action 1".to_string()],
    Severity::Warning
);
let backend = MockBackend::with_response(Ok(insight));

// Error simulation
let backend = MockBackend::error("Simulated failure".to_string());

// Multiple responses (cycles through)
let responses = vec![Ok(insight1), Err(error), Ok(insight2)];
let backend = MockBackend::with_responses(responses);
```

## Prompt Engineering

### System Context

All backends receive rich system context including:

- **Time Window**: Duration and event counts
- **Error Analysis**: Recent error and fault messages with timestamps
- **Resource Metrics**: CPU, GPU, memory usage and power consumption
- **Memory Pressure**: Current system memory state
- **Trigger Information**: Which rule fired and why

### Prompt Structure

The AI analyzer formats comprehensive prompts with:

1. **Expert Role**: "You are a macOS system diagnostics expert"
2. **System Context**: Quantified metrics and event counts
3. **Recent Errors**: Timestamped error messages with process details
4. **Recent Metrics**: Resource consumption data with trends
5. **Response Format**: Clear JSON schema specification
6. **Examples**: Sample response structure

### Response Format

All backends return structured insights:

```json
{
  "summary": "Brief description of the main issue",
  "root_cause": "Most likely underlying cause (or null)",
  "recommendations": ["Specific actionable step 1", "Step 2"],
  "severity": "info|warning|critical"
}
```

## Response Processing

### JSON Extraction

The system handles various LLM response formats:

- **Plain JSON**: Direct JSON responses
- **Markdown Code Blocks**: JSON wrapped in ```json blocks
- **Mixed Content**: JSON embedded in explanatory text
- **Malformed Responses**: Graceful error handling

### Error Handling

Comprehensive error handling for:

- **Network Failures**: HTTP timeouts and connection errors
- **Authentication**: Invalid API keys or permissions
- **Rate Limits**: API quota exceeded
- **Invalid Responses**: Malformed JSON or missing fields
- **Backend Errors**: LLM service unavailable

### Retry Logic

Built-in resilience features:

- **Timeout Handling**: 60-second request timeout
- **Error Classification**: Different handling for different error types
- **Graceful Degradation**: Continue operation with reduced functionality
- **Failure Tracking**: Monitor backend health and performance

## Performance Considerations

### Request Optimization

- **Low Temperature**: 0.1 for consistent analysis (not creative writing)
- **Token Limits**: 1000 tokens max for focused responses
- **Efficient Prompts**: Structured data format reduces token usage

### Caching Strategy

- **Context Reuse**: Similar events can reuse analysis patterns
- **Response Validation**: Ensure responses match expected schema
- **Fallback Handling**: Graceful degradation when backends unavailable

### Resource Usage

- **Memory**: Minimal overhead, responses processed immediately
- **CPU**: Async processing prevents blocking
- **Network**: Only for cloud backends, local backends use no bandwidth

## Security Considerations

### Data Privacy

- **Local Processing**: Ollama keeps all data on-device
- **No Persistence**: System data not stored permanently
- **Minimal Exposure**: Only relevant events sent to AI backends

### API Security

- **Key Management**: Secure storage of API credentials
- **HTTPS Only**: Encrypted communication for cloud backends
- **Request Validation**: Sanitize inputs before sending to backends

### Error Information

- **No Sensitive Data**: Error messages don't expose system details
- **Logging Controls**: Debug information can be disabled
- **Audit Trail**: Track backend usage without exposing data

## Testing

### Unit Tests

- **Response Parsing**: Validate JSON extraction and parsing
- **Error Handling**: Test various failure scenarios
- **Severity Mapping**: Ensure consistent severity interpretation

### Property-Based Tests

- **JSON Extraction**: Test with various response formats
- **Error Resilience**: Verify graceful handling of malformed data
- **Backend Consistency**: Ensure deterministic behavior

### Integration Tests

- **Live Backend Tests**: Marked `#[ignore]` to avoid CI failures
- **Mock Scenarios**: Comprehensive testing with MockBackend
- **End-to-End**: Full pipeline testing with real system events

## Troubleshooting

### Common Issues

**Ollama Connection Failed**:
```bash
# Check if Ollama is running
curl http://localhost:11434/api/tags

# Start Ollama service
ollama serve

# Pull required model
ollama pull llama3
```

**OpenAI Authentication Error**:
```bash
# Verify API key format
echo $OPENAI_API_KEY | grep "^sk-"

# Test API access
curl -H "Authorization: Bearer $OPENAI_API_KEY" \
     https://api.openai.com/v1/models
```

**Response Parsing Errors**:
- Check model compatibility (some models don't follow JSON format well)
- Verify prompt engineering for your specific model
- Enable debug logging to see raw responses

### Debug Commands

```bash
# Enable debug logging
RUST_LOG=debug cargo run

# Test specific backend
cargo test ai::backends --nocapture

# Run integration tests (requires backends)
cargo test --ignored ai::backends
```

## Future Enhancements

### Planned Features

- **Response Caching**: Cache similar analyses to reduce API calls
- **Model Selection**: Automatic model selection based on analysis complexity
- **Streaming Responses**: Real-time analysis updates for long-running queries
- **Custom Backends**: Plugin system for additional LLM providers

### Model Support

- **Local Models**: Additional Ollama model support
- **Cloud Providers**: Azure OpenAI, Anthropic Claude, Google Gemini
- **Specialized Models**: System-specific fine-tuned models
- **Hybrid Approaches**: Combine multiple models for better accuracy