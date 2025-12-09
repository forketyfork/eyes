# Configuration

Eyes is configured via a TOML file located at `~/.config/eyes/config.toml` by default.

## Configuration File Location

The application searches for configuration in this order:

1. Path specified via `--config` CLI flag
2. Path in `EYES_CONFIG` environment variable
3. `~/.config/eyes/config.toml` (default)
4. Built-in defaults if no file found

## Configuration Structure

### Logging Section

Controls which system logs are captured:

```toml
[logging]
# Predicate filter for log stream command
# Uses Apple's predicate syntax
predicate = "messageType == error OR messageType == fault"
```

**Common predicates:**
- `"messageType == error"` - Only errors
- `"messageType == fault"` - Only faults (critical errors)
- `"subsystem == 'com.apple.Safari'"` - Specific app logs
- `"process == 'kernel'"` - Kernel messages only

### Metrics Section

Controls resource monitoring frequency:

```toml
[metrics]
# How often to sample system metrics (seconds)
interval_seconds = 5
```

Lower intervals provide more granular data but increase CPU usage.

### Buffer Section

Controls the rolling event buffer:

```toml
[buffer]
# Maximum age of events to retain (seconds)
max_age_seconds = 60

# Maximum number of events to store
max_size = 1000
```

The buffer automatically prunes old events. Larger buffers provide more context for AI analysis but use more memory.

### Triggers Section

Controls when AI analysis is invoked:

```toml
[triggers]
# Number of errors to trigger analysis
error_threshold = 5

# Time window for error counting (seconds)
error_window_seconds = 10

# Memory pressure level to trigger analysis
# Options: "Normal", "Warning", "Critical"
memory_threshold = "Warning"
```

### AI Section

Configures the AI backend:

```toml
[ai]
# Backend type: "ollama" or "openai"
backend = "ollama"

# API endpoint (for Ollama)
endpoint = "http://localhost:11434"

# Model name
model = "llama3"

# For OpenAI backend:
# backend = "openai"
# api_key = "sk-..."
# model = "gpt-4"
```

### Alerts Section

Controls notification behavior:

```toml
[alerts]
# Maximum notifications per minute
rate_limit_per_minute = 3
```

Rate limiting prevents notification spam during cascading failures.

## Environment Variables

- `EYES_CONFIG`: Override default config file path
- `RUST_LOG`: Set logging level (e.g., `debug`, `info`, `warn`, `error`)

## Validation

Invalid configuration values fall back to safe defaults:
- Missing sections use built-in defaults
- Out-of-range values are clamped to valid ranges
- Invalid enum values (e.g., memory_threshold) use safe defaults
- Malformed TOML reports errors and uses all defaults
