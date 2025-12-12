# Configuration

Eyes is configured via a TOML file that can be loaded from any path. The configuration system provides sensible defaults for all values, making it easy to get started with minimal setup.

## Configuration File Location

The application loads configuration from a path specified via the `--config` CLI flag. If no configuration file is provided or if the file is missing optional values, built-in defaults are used.

```bash
# Use default configuration
eyes

# Load from specific file
eyes --config config.toml

# Load from custom path
eyes --config /path/to/my-config.toml
```

See [CLI Documentation](cli.md) for complete command-line usage.

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

The Mock backend provides canned responses for testing and development. It requires no additional configuration and always returns successful analysis results with predefined insights at **Info severity level**.

**Important**: Since the mock backend returns Info-level insights and only Critical insights trigger notifications, **no macOS notifications will be sent** when using the mock backend. This is by design to avoid notification spam during testing.

This backend is useful for:

- Testing the application logic without requiring a real LLM
- Development when network access is limited
- Automated testing scenarios where notifications aren't needed
- Demonstrating the system without AI dependencies

**For notification testing**: Use a local Ollama backend instead of the mock backend.

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

## Common Use Cases

### Development and Testing

For development environments where you want high sensitivity to catch issues early:

#### Option 1: Mock Backend (No Notifications)
```toml
[logging]
predicate = "messageType == error OR messageType == fault OR messageType == info"

[metrics]
interval_seconds = 2

[triggers]
error_threshold = 1
error_window_seconds = 5
memory_threshold = "Normal"

[ai]
backend = "mock"

[alerts]
rate_limit_per_minute = 10
```

This configuration:
- Captures more log types including info messages
- Samples metrics more frequently (every 2 seconds)
- Triggers analysis on the first error
- Uses mock AI backend to avoid external dependencies
- **Note**: Mock backend returns Info-level insights, so no notifications will be sent (only Critical insights trigger notifications)

#### Option 2: Local AI with Notifications
```toml
[logging]
predicate = "messageType == error OR messageType == fault"

[metrics]
interval_seconds = 2

[triggers]
error_threshold = 1
error_window_seconds = 5
memory_threshold = "Normal"

[ai]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"

[alerts]
rate_limit_per_minute = 10
```

This configuration:
- Uses local Ollama for realistic AI analysis that can return Critical insights
- Enables actual notification testing
- Requires Ollama to be installed and running locally

### Production Monitoring

For production systems where you want balanced monitoring without false positives:

```toml
[logging]
predicate = "messageType == error OR messageType == fault"

[metrics]
interval_seconds = 10

[triggers]
error_threshold = 10
error_window_seconds = 30
memory_threshold = "Critical"

[ai]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"

[alerts]
rate_limit_per_minute = 2
```

This configuration:
- Focuses on errors and faults only
- Reduces metrics frequency to save resources
- Requires more errors to trigger analysis (reduces noise)
- Only triggers on critical memory pressure
- Uses local AI for privacy
- Conservative notification rate

### High-Security Environment

For environments where security is paramount and data must stay local:

```toml
[logging]
predicate = "messageType == error AND (category == 'security' OR subsystem CONTAINS 'security')"

[metrics]
interval_seconds = 5

[triggers]
error_threshold = 1
error_window_seconds = 10
memory_threshold = "Warning"

[ai]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"

[alerts]
rate_limit_per_minute = 1
```

This configuration:
- Filters for security-related errors only
- Triggers analysis immediately on security issues
- Uses only local AI processing
- Very conservative notification rate

### Resource-Constrained Environment

For systems with limited CPU and memory resources:

```toml
[logging]
predicate = "messageType == fault"

[metrics]
interval_seconds = 30

[buffer]
max_age_seconds = 30
max_size = 500

[triggers]
error_threshold = 20
error_window_seconds = 60
memory_threshold = "Critical"

[ai]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "mistral"

[alerts]
rate_limit_per_minute = 1
```

This configuration:
- Only captures critical faults
- Reduces metrics frequency significantly
- Smaller buffer to save memory
- Higher error threshold to reduce AI calls
- Uses a lighter AI model
- Minimal notification rate

## Notification Behavior

The alert system only sends macOS notifications for insights with **Critical** severity. This prevents notification fatigue while ensuring you're alerted to the most important issues.

### Severity Levels and Notification Behavior

- **Critical**: Triggers macOS notifications (the only level that sends notifications)
- **Warning**: Logged but no notification sent
- **Info**: Logged but no notification sent

### AI Backend Notification Behavior

Different AI backends have different tendencies for severity assignment:

- **Ollama/OpenAI**: Can return any severity level based on actual analysis
- **Mock Backend**: Always returns Info-level insights (no notifications will be sent)

If you want to test notifications during development, use a local Ollama backend instead of the mock backend.

## Trigger Rule Customization

The trigger system uses built-in rules that can be customized through configuration. Understanding how these rules work helps you tune the system for your specific needs.

### Error Frequency Rule

Triggers when the number of error/fault log entries exceeds the threshold within the time window.

**Configuration:**
- `triggers.error_threshold`: Number of errors required
- `triggers.error_window_seconds`: Time window for counting errors

**Tuning Guidelines:**
- **High-traffic systems**: Increase threshold (10-50) to avoid noise
- **Critical systems**: Decrease threshold (1-3) for immediate detection
- **Bursty errors**: Use shorter windows (5-10 seconds)
- **Sustained issues**: Use longer windows (30-60 seconds)

### Memory Pressure Rule

Triggers when system memory pressure reaches or exceeds the configured level.

**Configuration:**
- `triggers.memory_threshold`: "Normal", "Warning", or "Critical"

**Tuning Guidelines:**
- **"Normal"**: Very sensitive, triggers on any memory pressure
- **"Warning"**: Balanced, triggers on moderate pressure (default)
- **"Critical"**: Conservative, only triggers on severe pressure

### Resource Spike Detection

The system automatically detects sudden increases in CPU and GPU usage. This behavior is built-in and cannot be disabled, but you can influence it indirectly:

**Indirect Configuration:**
- `metrics.interval_seconds`: Shorter intervals detect spikes faster
- `buffer.max_age_seconds`: Longer retention helps identify patterns

### Crash Detection

The system automatically looks for process crash indicators in log messages. This is a built-in rule that cannot be configured but is always active when monitoring error and fault messages.

**Related Configuration:**
- `logging.predicate`: Must include "fault" messages to detect crashes
- `triggers.error_threshold`: Crashes often generate multiple log entries

## Advanced Predicate Filtering

The `logging.predicate` field uses Apple's predicate syntax for powerful log filtering. Here are advanced examples:

### Application-Specific Monitoring

```toml
[logging]
# Monitor specific application
predicate = "subsystem == 'com.apple.Safari' AND messageType == error"

# Monitor multiple applications
predicate = "subsystem IN {'com.apple.Safari', 'com.apple.Mail'} AND messageType == error"

# Monitor all Apple applications
predicate = "subsystem BEGINSWITH 'com.apple.' AND messageType == error"
```

### Category-Based Filtering

```toml
[logging]
# Security-related events only
predicate = "category == 'security' OR category == 'authentication'"

# Network-related issues
predicate = "category CONTAINS 'network' AND messageType == error"

# System-level issues
predicate = "category IN {'kernel', 'system', 'hardware'} AND messageType != info"
```

### Process-Based Monitoring

```toml
[logging]
# Monitor system processes only
predicate = "process IN {'kernel', 'launchd', 'WindowServer'} AND messageType == error"

# Exclude noisy processes
predicate = "messageType == error AND process != 'mdworker' AND process != 'mds'"

# Monitor processes by pattern
predicate = "process BEGINSWITH 'com.apple.' AND messageType == fault"
```

### Time-Based Filtering

```toml
[logging]
# Recent events only (last hour)
predicate = "messageType == error AND timestamp >= now() - 3600"

# Exclude very recent events (avoid duplicates)
predicate = "messageType == error AND timestamp < now() - 5"
```

### Complex Combinations

```toml
[logging]
# Comprehensive system monitoring
predicate = """
(messageType == error OR messageType == fault) AND
(category IN {'security', 'kernel', 'system'} OR 
 subsystem BEGINSWITH 'com.apple.') AND
process != 'mdworker'
"""
```

## Error Handling

Configuration loading can fail with these error types:

- **`ConfigError::ReadError`**: File cannot be read (missing, permissions, etc.)
- **`ConfigError::TomlError`**: TOML syntax is malformed
- **`ConfigError::ValidationError`**: Configuration values are invalid

All errors include descriptive messages to help diagnose the issue.

### Common Configuration Errors

**Invalid TOML syntax:**
```
Error: TOML parsing failed: expected an equals, found an identifier at line 5
```

**Validation failures:**
```
Error: Configuration validation failed: triggers.error_threshold must be at least 1
Error: Configuration validation failed: ai.api_key cannot be empty
```

**File access issues:**
```
Error: Failed to read config file 'config.toml': No such file or directory
Error: Failed to read config file 'config.toml': Permission denied
```
