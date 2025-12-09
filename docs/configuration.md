# Configuration

Eyes is configured via a TOML file that can be loaded from any path. The configuration system provides sensible defaults for all values, making it easy to get started with minimal setup.

## Configuration File Location

The application loads configuration from a path specified at runtime (typically via CLI flag). If no configuration file is provided or if the file is missing optional values, built-in defaults are used.

## Configuration Structure

All configuration fields are optional. If a field is omitted, a safe default value is used automatically.

### Complete Example

```toml
# Log stream predicate filter (Apple's query language)
log_predicate = "messageType == error OR messageType == fault"

# Metrics sampling interval (seconds)
metrics_interval_secs = 5

# Rolling buffer configuration
buffer_max_age_secs = 60
buffer_max_size = 1000

# Trigger thresholds
error_threshold = 5
error_window_secs = 10
memory_threshold = "Warning"

# Alert rate limiting
alert_rate_limit = 3

# AI backend configuration
[ai_backend]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"
```

## Configuration Fields

### Log Filtering

**`log_predicate`** (string, default: `"messageType == error OR messageType == fault"`)

Predicate filter for the `log stream` command using Apple's query language.

**Common predicates:**
- `"messageType == error"` - Only errors
- `"messageType == fault"` - Only faults (critical errors)
- `"subsystem == 'com.apple.Safari'"` - Specific app logs
- `"process == 'kernel'"` - Kernel messages only
- Combine with `AND`, `OR`, `NOT` operators

### Metrics Collection

**`metrics_interval_secs`** (u64, default: `5`, minimum: `1`)

Interval between metrics samples in seconds. Lower values provide more granular data but increase CPU usage.

### Rolling Buffer

**`buffer_max_age_secs`** (u64, default: `60`, minimum: `1`)

Maximum age of events to retain in seconds. Events older than this are automatically pruned.

**`buffer_max_size`** (usize, default: `1000`, minimum: `1`)

Maximum number of events to store in the buffer. When capacity is reached, oldest events are removed.

### Trigger Thresholds

**`error_threshold`** (usize, default: `5`, minimum: `1`)

Number of error/fault log entries within the time window required to trigger AI analysis.

**`error_window_secs`** (u64, default: `10`, minimum: `1`)

Time window in seconds for counting errors toward the threshold.

**`memory_threshold`** (MemoryPressure, default: `"Warning"`)

Memory pressure level that triggers AI analysis. Valid values:
- `"Normal"` - No memory pressure
- `"Warning"` - System is under memory pressure
- `"Critical"` - System is critically low on memory

### Alert Rate Limiting

**`alert_rate_limit`** (usize, default: `3`, minimum: `1`)

Maximum number of notifications to send per minute. Prevents alert fatigue during cascading failures.

### AI Backend

The `ai_backend` section configures which LLM backend to use. It uses a tagged enum format with the `backend` field determining the variant.

#### Ollama Backend (Local)

```toml
[ai_backend]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"
```

**`endpoint`** (string, default: `"http://localhost:11434"`)
- Ollama API endpoint URL
- Must not be empty

**`model`** (string, default: `"llama3"`)
- Model name to use for analysis
- Must not be empty
- Common options: `"llama3"`, `"mistral"`, `"codellama"`

#### OpenAI Backend (Cloud)

```toml
[ai_backend]
backend = "openai"
api_key = "sk-..."
model = "gpt-4"
```

**`api_key`** (string, required)
- OpenAI API key
- Must not be empty
- Keep this secret and never commit to version control

**`model`** (string, default: `"gpt-4"`)
- Model name to use for analysis
- Must not be empty
- Common options: `"gpt-4"`, `"gpt-4-turbo"`, `"gpt-3.5-turbo"`

## Validation

The configuration system validates all values when loading:

- **Zero values**: Numeric fields that must be at least 1 are validated
- **Empty strings**: Required string fields (endpoints, models, API keys) cannot be empty
- **Enum values**: Memory pressure must be a valid variant
- **File errors**: Missing files or malformed TOML produce clear error messages

If validation fails, the application returns a `ConfigError` with a descriptive message.

## Default Behavior

If no configuration file is provided, or if specific fields are omitted, the following defaults are used:

```rust
Config {
    log_predicate: "messageType == error OR messageType == fault",
    metrics_interval_secs: 5,
    buffer_max_age_secs: 60,
    buffer_max_size: 1000,
    error_threshold: 5,
    error_window_secs: 10,
    memory_threshold: MemoryPressure::Warning,
    alert_rate_limit: 3,
    ai_backend: AIBackendConfig::Ollama {
        endpoint: "http://localhost:11434",
        model: "llama3",
    },
}
```

These defaults are production-ready and suitable for most use cases.

## Helper Methods

The `Config` struct provides convenience methods for working with durations:

```rust
config.metrics_interval()  // Returns Duration
config.buffer_max_age()    // Returns Duration
config.error_window()      // Returns Duration
```

## Loading Configuration

```rust
use eyes::config::Config;
use std::path::Path;

// Load from file
let config = Config::from_file(Path::new("config.toml"))?;

// Use defaults
let config = Config::default();

// Create new (same as default)
let config = Config::new();
```

## Error Handling

Configuration loading can fail with these error types:

- **`ConfigError::ReadError`**: File cannot be read (missing, permissions, etc.)
- **`ConfigError::TomlError`**: TOML syntax is malformed
- **`ConfigError::ValidationError`**: Configuration values are invalid

All errors include descriptive messages to help diagnose the issue.
