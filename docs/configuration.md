# Configuration

Eyes is configured via a TOML file that can be loaded from any path. The configuration system provides sensible defaults for all values, making it easy to get started with minimal setup.

## Configuration File Location

The application loads configuration from a path specified at runtime (typically via CLI flag). If no configuration file is provided or if the file is missing optional values, built-in defaults are used.

## Configuration Structure

All configuration fields are optional. If a field is omitted, a safe default value is used automatically.

The configuration is organized into logical sections: `logging`, `metrics`, `buffer`, `triggers`, `ai`, and `alerts`.

### Complete Example

```toml
[logging]
predicate = "messageType == error OR messageType == fault"

[metrics]
interval_seconds = 5

[buffer]
max_age_seconds = 60
max_size = 1000

[triggers]
error_threshold = 5
error_window_seconds = 10
memory_threshold = "Warning"

[alerts]
rate_limit_per_minute = 3

[ai]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"
```

## Configuration Fields

### Logging Section

**`[logging]`**

Controls log stream collection behavior.

**`predicate`** (string, default: `"messageType == error OR messageType == fault"`)

Predicate filter for the `log stream` command using Apple's query language.

**Common predicates:**
- `"messageType == error"` - Only errors
- `"messageType == fault"` - Only faults (critical errors)
- `"subsystem == 'com.apple.Safari'"` - Specific app logs
- `"process == 'kernel'"` - Kernel messages only
- Combine with `AND`, `OR`, `NOT` operators

### Metrics Section

**`[metrics]`**

Controls system metrics collection.

**`interval_seconds`** (u64, default: `5`, minimum: `1`)

Interval between metrics samples in seconds. Lower values provide more granular data but increase CPU usage.

### Buffer Section

**`[buffer]`**

Controls the rolling event buffer that stores recent logs and metrics.

**`max_age_seconds`** (u64, default: `60`, minimum: `1`)

Maximum age of events to retain in seconds. Events older than this are automatically pruned.

**`max_size`** (usize, default: `1000`, minimum: `1`)

Maximum number of events to store in the buffer. When capacity is reached, oldest events are removed.

### Triggers Section

**`[triggers]`**

Controls when AI analysis is triggered.

**`error_threshold`** (usize, default: `5`, minimum: `1`)

Number of error/fault log entries within the time window required to trigger AI analysis.

**`error_window_seconds`** (u64, default: `10`, minimum: `1`)

Time window in seconds for counting errors toward the threshold.

**`memory_threshold`** (MemoryPressure, default: `"Warning"`)

Memory pressure level that triggers AI analysis. Valid values:
- `"Normal"` - No memory pressure
- `"Warning"` - System is under memory pressure
- `"Critical"` - System is critically low on memory

### Alerts Section

**`[alerts]`**

Controls notification delivery and rate limiting.

**`rate_limit_per_minute`** (usize, default: `3`, minimum: `1`)

Maximum number of notifications to send per minute. Prevents alert fatigue during cascading failures.

### AI Section

**`[ai]`**

Configures which LLM backend to use for analysis. The section uses a tagged enum format with the `backend` field determining the variant.

#### Ollama Backend (Local)

```toml
[ai]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"
```

**`backend`** (string, required: `"ollama"`)

**`endpoint`** (string, default: `"http://localhost:11434"`)
- Ollama API endpoint URL
- Must not be empty

**`model`** (string, default: `"llama3"`)
- Model name to use for analysis
- Must not be empty
- Common options: `"llama3"`, `"mistral"`, `"codellama"`

#### OpenAI Backend (Cloud)

```toml
[ai]
backend = "openai"
api_key = "sk-..."
model = "gpt-4"
```

**`backend`** (string, required: `"openai"`)

**`api_key`** (string, required)
- OpenAI API key
- Must not be empty
- Keep this secret and never commit to version control

**`model`** (string, default: `"gpt-4"`)
- Model name to use for analysis
- Must not be empty
- Common options: `"gpt-4"`, `"gpt-4-turbo"`, `"gpt-3.5-turbo"`

#### Mock Backend (Testing)

```toml
[ai]
backend = "mock"
```

**`backend`** (string, required: `"mock"`)

Mock backend for testing and development. No additional configuration required. The mock backend provides canned responses for AI analysis without requiring an actual LLM service, making it ideal for:

- Unit testing and integration testing
- Development environments without LLM access
- Offline development scenarios
- CI/CD pipelines that don't need real AI analysis

#### Mock Backend (Testing)

```toml
[ai]
backend = "mock"
```

**`backend`** (string, required: `"mock"`)

The Mock backend provides canned responses for testing and development. It requires no additional configuration and always returns successful analysis results with predefined insights. This backend is useful for:

- Testing the application without requiring a real LLM
- Development when network access is limited
- Automated testing scenarios
- Demonstrating the system without AI dependencies

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
    logging: LoggingConfig {
        predicate: "messageType == error OR messageType == fault",
    },
    metrics: MetricsConfig {
        interval_seconds: 5,
    },
    buffer: BufferConfig {
        max_age_seconds: 60,
        max_size: 1000,
    },
    triggers: TriggersConfig {
        error_threshold: 5,
        error_window_seconds: 10,
        memory_threshold: MemoryPressure::Warning,
    },
    ai: AIConfig {
        backend: AIBackendConfig::Ollama {
            endpoint: "http://localhost:11434",
            model: "llama3",
        },
    },
    alerts: AlertsConfig {
        rate_limit_per_minute: 3,
    },
}
```

These defaults are production-ready and suitable for most use cases.

## Helper Methods

The `Config` struct provides convenience methods for working with durations:

```rust
config.metrics.interval_seconds  // u64
config.buffer.max_age_seconds    // u64
config.triggers.error_window_seconds  // u64
```

## Loading Configuration

### Direct Configuration Loading

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

### Application-Level Loading

The `SystemObserver` provides a convenient wrapper for configuration loading:

```rust
use eyes::SystemObserver;

// Load from file with fallback to defaults
let config = SystemObserver::load_config(Some("config.toml"))?;

// Use defaults only
let config = SystemObserver::load_config(None)?;
```

This approach handles missing files gracefully by falling back to default configuration.

## Error Handling

Configuration loading can fail with these error types:

- **`ConfigError::ReadError`**: File cannot be read (missing, permissions, etc.)
- **`ConfigError::TomlError`**: TOML syntax is malformed
- **`ConfigError::ValidationError`**: Configuration values are invalid

All errors include descriptive messages to help diagnose the issue.
