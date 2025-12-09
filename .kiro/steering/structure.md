# Project Structure

## Module Organization

The codebase follows a modular architecture with clear separation of concerns:

```
src/
├── main.rs              # Application entry point
├── lib.rs               # Public API and module exports
├── error.rs             # Centralized error types
├── config/              # Configuration management
├── collectors/          # Data source implementations
├── aggregator/          # Event buffering and aggregation
├── triggers/            # Rule engine for AI activation
├── ai/                  # AI analysis and backend integrations
└── alerts/              # Notification and rate limiting
```

## Module Responsibilities

### `collectors/`
Interfaces with macOS system tools to gather data:
- `log_collector.rs`: Streams and parses Unified Logs via `log stream`
- `metrics_collector.rs`: Gathers resource metrics via `powermetrics`

### `aggregator/`
Manages rolling buffers and event aggregation:
- `event_aggregator.rs`: Time-windowed event storage for AI analysis

### `triggers/`
Determines when to invoke AI analysis:
- `trigger_engine.rs`: Core trigger evaluation logic
- `rules.rs`: Configurable trigger rule definitions

### `ai/`
AI integration layer:
- `analyzer.rs`: Coordinates AI analysis and insight generation
- `backends.rs`: LLM backend implementations (Ollama, OpenAI)

### `alerts/`
User notification system:
- `alert_manager.rs`: Delivers macOS notifications
- `rate_limiter.rs`: Prevents alert fatigue

### `config/`
Application configuration:
- `config.rs`: Configuration structures and loading logic

## Code Conventions

### Module Structure
- Each module has a `mod.rs` that re-exports public types
- Implementation details stay in separate files
- Use `pub use` in `mod.rs` for clean public APIs

### Error Handling
- Custom error types defined in `error.rs`
- Use `thiserror` for error type definitions
- Use `anyhow` for application-level error propagation

### Async Patterns
- All I/O operations use Tokio async runtime
- Collectors run as continuous async streams
- Use `tokio::spawn` for concurrent tasks

### Testing
- Unit tests in same file as implementation
- Integration tests use `mockall` for external dependencies
- Property-based tests for complex logic validation

### Documentation
- the README.md file should be the "face" of the project: concise, high-level.
- technical details should be kept in the docs/ files; they should be small and atomic (one file per concept, idea, technical decision).
