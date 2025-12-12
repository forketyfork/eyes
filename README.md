# Eyes - macOS System Observer

An AI-native monitoring tool for macOS that provides real-time insights into system runtime behavior through intelligent log analysis and resource tracking.

## Overview

Eyes monitors your Mac's health by streaming system logs and metrics, using AI to diagnose issues before they become critical. Unlike passive monitoring dashboards, Eyes proactively identifies problems and delivers actionable insights through native macOS notifications.

## Features

- **Real-time Log Monitoring**: Streams macOS Unified Logs with intelligent predicate filtering
- **Resource Tracking**: Monitors CPU, memory, GPU, and energy consumption via `powermetrics`
- **AI-Powered Diagnostics**: Deep integration with local LLMs (Ollama) or cloud APIs (OpenAI)
- **Smart Alerting**: Rate-limited native notifications to prevent alert fatigue
- **Privacy-First**: Designed to run locally with Ollama—your system data never leaves your machine

## Quick Start

### Prerequisites

- macOS 10.15 or later
- Rust 2021 edition or later
- (Optional) [Ollama](https://ollama.ai) for local AI analysis
- (Optional) sudo privileges for enhanced metrics via `powermetrics`

### Installation

```bash
# Clone the repository
git clone <repository-url>
cd eyes

# Build the project
cargo build --release

# Run the application
cargo run --release
```

### Configuration

Create a configuration file (e.g., `config.toml`):

```toml
# All fields are optional - defaults are provided
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

Or use the built-in defaults by running without a config file. See [Configuration](docs/configuration.md) for all options.

## Architecture

Eyes uses a multi-threaded producer-consumer architecture:

```
Log Stream → Event Aggregator → Trigger Engine → AI Analyzer → Alert Manager
Metrics    ↗                                                   ↓
                                                        macOS Notifications
```

### Components

- **Log Collector**: Interfaces with `log stream` to capture system logs
- **Metrics Collector**: Gathers resource data via `powermetrics`
- **Event Aggregator**: Maintains rolling buffers of recent events
- **Trigger Engine**: Applies heuristic rules to determine when AI analysis is needed
- **AI Analyzer**: Coordinates analysis with LLM backends and generates actionable insights
- **Alert Manager**: Delivers rate-limited native notifications

## Development

### Build Commands

```bash
# Build the project
cargo build

# Run tests
cargo test

# Run with verbose logging
RUST_LOG=debug cargo run

# Format code
cargo fmt

# Lint with clippy
cargo clippy
```

### Testing

The project uses multiple testing strategies:

- **Unit Tests**: Fast, isolated tests with mocked dependencies
- **Property-Based Tests**: Uses `quickcheck` to verify correctness properties
- **Integration Tests**: End-to-end testing with actual system tools

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run property-based tests with more iterations
cargo test --release
```

## Permissions

Eyes requires specific macOS permissions:

- **Full Disk Access**: Required to read Unified Logs (System Preferences → Security & Privacy → Privacy → Full Disk Access)
- **Notifications**: Requested automatically on first alert
- **Sudo Access**: Optional, for enhanced metrics via `powermetrics`

## AI Backend Options

### Ollama (Recommended)

Local LLM execution for complete privacy:

```bash
# Install Ollama
brew install ollama

# Pull a model
ollama pull llama3

# Start Ollama service
ollama serve
```

### OpenAI

Cloud-based alternative for enhanced capabilities:

```toml
[ai]
backend = "openai"
api_key = "sk-..."
model = "gpt-4"
```

## Documentation

- [Architecture](docs/architecture.md) - System design and threading model
- [Data Models](docs/data-models.md) - Core event types and structures
- [Event Aggregation](docs/event-aggregation.md) - Rolling buffer implementation and usage
- [Configuration](docs/configuration.md) - Configuration options and examples
- [Subprocess Management](docs/subprocess-management.md) - Process lifecycle and error handling
- [Buffer Parsing](docs/buffer-parsing.md) - Stream processing and data parsing strategies
- [macOS Integration](docs/macos-integration.md) - System permissions and tools
- [AI Backends](docs/ai-backends.md) - LLM integration details
- [Trigger Rules](docs/trigger-rules.md) - Built-in trigger rules and customization
- [Testing](docs/testing.md) - Testing strategy and guidelines

## Project Status

This project is currently in active development. See `.kiro/specs/macos-system-observer/tasks.md` for the implementation roadmap.

**Completed:**
- ✅ Project structure and dependencies
- ✅ Core data models and event structures (LogEvent, MetricsEvent, enums)
- ✅ Configuration management with TOML parsing and validation
- ✅ Event aggregation with rolling buffers (time-based expiration and capacity limits)
- ✅ Log stream collector with subprocess management, automatic restart, and non-blocking I/O
- ✅ Metrics collector with powermetrics integration, robust fallback monitoring, graceful degradation, and advanced buffer parsing for split JSON handling
- ✅ Trigger engine with built-in rules (error frequency, memory pressure, crash detection, resource spikes)
- ✅ AI analysis coordinator with insight generation and backend abstraction

## License

[License information to be added]

## Contributing

[Contributing guidelines to be added]
