# Repository Guidelines

This guide keeps contributions consistent for Eyes, a Rust macOS system observer.

## Project Structure & Modules
- `src/main.rs` is the entry point; wiring is incremental while components land.
- `src/collectors` handles log/metrics ingestion for macOS tools (`log stream`, `powermetrics`).
- `src/aggregator` maintains rolling buffers; `src/triggers` applies rules; `src/ai` formats prompts and talks to backends (Ollama/OpenAI); `src/alerts` manages notifications with rate limiting; `src/config` parses TOML; shared types live in `src/events.rs`, errors in `src/error.rs`.
- Architecture and reference docs live in `docs/` (architecture, data-models, event-aggregation, testing); keep them updated when behavior shifts.
- Local config is an optional TOML file (e.g., `config.toml`) placed next to the binary; defaults cover missing fields.

## Build, Test, and Development Commands
- `cargo build` / `cargo run --release` to compile or run; set `RUST_LOG=debug` for verbose traces.
- `cargo test` for unit/property tests; `cargo test -- --nocapture` to see logs; `cargo test -- --ignored` for integration/macOS-only cases; use `--release` to increase `quickcheck` iterations.
- `cargo fmt` before committing; `cargo clippy -- -D warnings` to catch lint issues early.

## Coding Style & Naming Conventions
- Rust 2021 edition; default `rustfmt` (4-space indent, ~100 column width).
- Prefer self-documenting code; reserve comments for rationale or non-obvious macOS constraints.
- Use `snake_case` for functions/fields, `CamelCase` for types/enums, `SCREAMING_SNAKE_CASE` for constants.
- Handle errors with `anyhow`/`thiserror`; avoid panics in non-test code.

## Testing Guidelines
- Place fast unit tests in the same module under `#[cfg(test)]`; use property tests (`quickcheck`) for parsers and aggregators.
- Cover new triggers, rate limiting, and AI prompt formatting with targeted cases; add ignored tests for macOS-only flows.
- Keep tests deterministic; gate long-running or privileged scenarios with `#[ignore]` and document the requirement.

## Commit & Pull Request Guidelines
- Commit messages: imperative mood; prefer Conventional Commits (`feat:`, `fix:`, `chore:`). Keep scope tight and explain the behavior change.
- PRs should summarize intent, list test commands run, and call out macOS permissions or config needed to verify.
- Link issues when available; include logs or sample config snippets that reviewers can reuse.
- Run `cargo fmt`, `cargo clippy`, and `cargo test` before requesting review; avoid adding commented-out code.
