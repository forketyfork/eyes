# Technology Stack

## Language & Runtime

- **Rust 2021 Edition**: Primary language for performance and safety
- **Tokio**: Async runtime with full feature set for concurrent operations

## Key Dependencies

### Core Libraries
- `serde` + `serde_json`: Serialization for JSON log parsing
- `plist`: macOS property list parsing for `powermetrics` output
- `toml`: Configuration file format
- `chrono`: Date/time handling with serde support
- `anyhow` + `thiserror`: Error handling patterns

### System Integration
- `reqwest`: HTTP client for AI backend communication
- `clap`: CLI argument parsing with derive macros
- `log` + `env_logger`: Structured logging

### Testing
- `quickcheck` + `quickcheck_macros`: Property-based testing
- `mockall`: Mocking framework for unit tests
- `tempfile`: Temporary file handling in tests

## macOS System Tools

The application interfaces with native macOS commands:
- `log stream`: Unified Logging System data source
- `powermetrics`: System resource metrics (requires sudo)
- `osascript`: Native notification delivery

## Build Commands

```bash
# Build the project
cargo build

# Build optimized release
cargo build --release

# Run the application
cargo run

# Run tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Check code without building
cargo check

# Format code
cargo fmt

# Lint with clippy
cargo clippy
```

## AI Backend Options

- **Ollama** (recommended): Local LLM execution for privacy
- **OpenAI API**: Cloud-based alternative for enhanced capabilities
