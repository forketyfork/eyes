# Eyes - macOS System Observer

An AI-native monitoring tool for macOS that provides real-time insights into system runtime behavior through intelligent log analysis and resource tracking.

## Overview

Eyes monitors your Mac's health by streaming system logs and metrics, using AI to diagnose issues before they become critical. Unlike passive monitoring dashboards, Eyes proactively identifies problems and delivers actionable insights through native macOS notifications.

## Features

- **Real-time Log Monitoring**: Streams macOS Unified Logs with intelligent predicate filtering
- **Resource Tracking**: Monitors CPU, memory, GPU, disk I/O, and energy consumption via `powermetrics` and `iostat`
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

# Run the application (uses default configuration)
cargo run --release

# Or run with custom configuration and verbose logging
cargo run --release -- --config config.toml --verbose
```

### Configuration

Create a configuration file (e.g., `config.toml`) or copy from the example:

```bash
cp config.example.toml config.toml
```

The configuration supports three AI backends:

```toml
# Ollama (Local - Recommended)
[ai]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"

# OpenAI (Cloud)
[ai]
backend = "openai"
api_key = "sk-..."
model = "gpt-4"

# Mock (Testing)
[ai]
backend = "mock"
```

Or use the built-in defaults by running without a config file. Eyes gracefully handles missing configuration files by falling back to safe defaults with warnings. See [Configuration](docs/configuration.md) for all options.

## Architecture

Eyes uses a multi-threaded producer-consumer architecture:

```
Log Stream → Event Aggregator → Trigger Engine → AI Analyzer → Alert Manager
Metrics    ↗                                                   ↓
Disk I/O   ↗                                           macOS Notifications
```

### Components

- **Log Collector**: Interfaces with `log stream` to capture system logs
- **Metrics Collector**: Gathers resource data via `powermetrics`
- **Disk Collector**: Monitors disk I/O activity via `iostat` and filesystem events
- **Event Aggregator**: Maintains rolling buffers of recent events
- **Trigger Engine**: Applies heuristic rules to determine when AI analysis is needed
- **AI Analyzer**: Coordinates analysis with LLM backends and generates actionable insights
- **Alert Manager**: Delivers rate-limited native notifications with intelligent queueing, async processing, and self-monitoring integration

## Command Line Interface

Eyes provides a comprehensive CLI for configuration and operation:

```bash
# Show help and available options
cargo run -- --help

# Run with custom configuration (gracefully falls back to defaults if missing)
cargo run -- --config config.toml

# Enable verbose logging
cargo run -- --verbose

# Combine options
cargo run -- --config config.toml --verbose
```

See [CLI Documentation](docs/cli.md) for complete usage details.

## Development

### Build Commands

```bash
# Build the project
cargo build

# Run tests
cargo test

# Run with verbose logging (via CLI flag)
cargo run -- --verbose

# Run with environment variable (alternative)
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

### Mock (Testing)

For testing and development without AI dependencies:

```toml
[ai]
backend = "mock"
```

The Mock backend provides canned responses and requires no external services, making it ideal for testing, development, and demonstrations.

## Documentation

- [CLI](docs/cli.md) - Command-line interface and usage examples
- [Architecture](docs/architecture.md) - System design and threading model
- [Application Orchestration](docs/application-orchestration.md) - Main application structure and component coordination
- [Data Models](docs/data-models.md) - Core event types and structures
- [Event Aggregation](docs/event-aggregation.md) - Rolling buffer implementation and usage
- [Configuration](docs/configuration.md) - Configuration options and examples
- [Subprocess Management](docs/subprocess-management.md) - Process lifecycle and error handling
- [Buffer Parsing](docs/buffer-parsing.md) - Stream processing and data parsing strategies
- [Disk Monitoring](docs/disk-monitoring.md) - Disk I/O activity monitoring and analysis
- [macOS Integration](docs/macos-integration.md) - System permissions and tools
- [AI Analysis](docs/ai-analysis.md) - AI-powered system diagnostics and insight generation
- [AI Backends](docs/ai-backends.md) - LLM integration details and backend implementations
- [Trigger Rules](docs/trigger-rules.md) - Built-in trigger rules and customization
- [Alerts](docs/alerts.md) - Notification system and rate limiting
- [Self-Monitoring](docs/self-monitoring.md) - Application performance metrics and health monitoring
- [Error Handling](docs/error-handling.md) - Resilience patterns and retry mechanisms
- [Async Processing](docs/async-processing.md) - Async/await patterns and concurrency
- [Testing](docs/testing.md) - Testing strategy and guidelines

## Project Status

This project is currently in active development. See `.kiro/specs/macos-system-observer/tasks.md` for the implementation roadmap.

**Completed:**
- ✅ Project structure and dependencies
- ✅ Core data models and event structures (LogEvent, MetricsEvent, enums)
- ✅ Configuration management with TOML parsing and validation
- ✅ Event aggregation with rolling buffers (time-based expiration and capacity limits)
- ✅ Log stream collector with subprocess management, automatic restart, non-blocking I/O, and comprehensive startup logging
- ✅ Metrics collector with powermetrics integration, graceful degradation, and advanced buffer parsing for plist format
- ✅ Trigger engine with built-in rules (error frequency, memory pressure, crash detection, resource spikes with running minimum algorithm)
- ✅ AI analysis coordinator with comprehensive prompt formatting, insight generation, and intelligent retry queue for failed requests with exponential backoff
- ✅ LLM backend implementations (Ollama for local inference, OpenAI for cloud-based analysis)
- ✅ Advanced JSON extraction from LLM responses with markdown and text parsing
- ✅ Mock backend for testing and development with configurable responses and failure simulation
- ✅ Alert system with rate-limited macOS notifications, intelligent spam prevention, alert queueing, async processing capabilities, and self-monitoring integration
- ✅ Comprehensive property-based testing with quickcheck for all major components
- ✅ UTF-8 safe text truncation for notification content limits
- ✅ Advanced resource spike detection using running minimum algorithm for transient spike capture
- ✅ **Checkpoint 1**: All core components implemented and tested (175 tests passing, 0 failures)
- ✅ **Main application orchestration**: SystemObserver struct with component initialization and configuration loading
- ✅ **Command-line interface**: Full CLI implementation with clap, argument validation, and help system
- ✅ **Self-monitoring system**: Application performance metrics collection including memory usage, event processing rates, AI analysis latency, and notification delivery success rates with comprehensive thread-safe integration across all components
- ✅ **Disk monitoring**: DiskCollector implementation with iostat integration, adaptive sampling, graceful degradation, and comprehensive buffer parsing for disk I/O metrics

**In Progress:**
- 🔄 Thread spawning and coordination (next: implement start/stop methods and event flow)

## License

[License information to be added]

## Contributing

[Contributing guidelines to be added]
