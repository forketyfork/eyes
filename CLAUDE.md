# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Eyes is a macOS system observer that monitors system logs and metrics, using AI (Ollama/OpenAI) to diagnose issues and deliver native notifications.

## Build and Test Commands

```bash
cargo build                     # Build
cargo run --release             # Run
cargo test                      # Unit and property tests
cargo test -- --nocapture       # Tests with output
cargo test -- --ignored         # macOS-only integration tests
cargo test --release            # Property tests with more quickcheck iterations
cargo test test_name            # Run specific test
cargo fmt                       # Format code
cargo clippy -- -D warnings     # Lint
RUST_LOG=debug cargo run        # Verbose logging
```

## Architecture

Multi-threaded producer-consumer pipeline:

```
Log Stream (log stream) ──┐
                          ├──► Event Aggregator ──► Trigger Engine ──► AI Analyzer ──► Alert Manager ──► Notifications
Metrics (powermetrics) ───┘    (rolling buffer)     (thresholds)       (Ollama/OpenAI)  (rate-limited)
```

Threads communicate via `mpsc` channels. The Event Aggregator maintains rolling buffers with time-based expiration (`buffer_max_age_secs`) and capacity limits (`buffer_max_size`).

## Module Structure

- `src/main.rs` - Entry point and component wiring
- `src/events.rs` - Core types: `LogEvent`, `MetricsEvent`, `MessageType`, `MemoryPressure`, `Severity`
- `src/error.rs` - Error types
- `src/config/` - TOML configuration parsing and validation
- `src/collectors/` - Log and metrics ingestion from macOS tools
- `src/aggregator/` - Rolling buffers for events
- `src/triggers/` - Threshold rules and trigger engine
- `src/ai/` - LLM backend abstraction and prompt formatting
- `src/alerts/` - Notification delivery with rate limiting

## Testing

- Unit tests in module files under `#[cfg(test)]`
- Property-based tests using `quickcheck` for parsers and aggregators
- Integration tests marked `#[ignore]` require macOS and permissions

## Coding Conventions

- Rust 2021 edition, default `rustfmt`
- `snake_case` for functions/fields, `CamelCase` for types/enums, `SCREAMING_SNAKE_CASE` for constants
- Error handling with `anyhow`/`thiserror`; no panics in non-test code
- Prefer self-documenting code; reserve comments for rationale or macOS-specific constraints

## Commit Style

Conventional Commits: `feat:`, `fix:`, `chore:`. Imperative mood, tight scope. Run `cargo fmt`, `cargo clippy`, and `cargo test` before committing.
