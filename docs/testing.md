# Testing Strategy

Eyes uses multiple testing approaches to ensure correctness and reliability.

## Test Types

### Unit Tests

Fast, isolated tests with mocked dependencies.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_event_parsing() {
        let json = r#"{"timestamp": "...", "messageType": "Error"}"#;
        let event = parse_log_event(json).unwrap();
        assert_eq!(event.message_type, MessageType::Error);
    }
}
```

Run with:
```bash
cargo test
```

### Property-Based Tests

Uses `quickcheck` to verify correctness properties with randomly generated inputs.

```rust
#[cfg(test)]
mod properties {
    use quickcheck_macros::quickcheck;

    // Feature: macos-system-observer, Property 1: Log parsing preserves structure
    #[quickcheck]
    fn prop_log_parsing_preserves_structure(
        timestamp: String,
        message: String
    ) -> bool {
        // Generate valid JSON log entry
        // Parse it
        // Verify all fields preserved
    }
}
```

Run with:
```bash
cargo test --release  # Release mode for more iterations
```

### Integration Tests

End-to-end testing with actual system tools (when available).

```rust
#[test]
#[cfg(target_os = "macos")]
#[ignore]  // Requires macOS and permissions
fn test_log_stream_integration() {
    let collector = LogCollector::new("messageType == error".to_string());
    // Spawn actual log stream process
    // Verify events are captured
}
```

**Platform-Specific Property Tests**: Some property-based tests that interact with real system processes are marked with `#[cfg(target_os = "macos")]` and `#[ignore]` to prevent spawning subprocesses during normal test runs:

```rust
#[quickcheck]
#[cfg(target_os = "macos")]
#[ignore]
fn prop_collector_state_management_on_failure(_scenario: SubprocessFailureScenario) -> bool {
    // Test spawns real processes so it's ignored by default
    // Verifies collector handles subprocess failures gracefully
}
```

Run integration tests with:
```bash
# Run ignored tests (requires macOS)
cargo test -- --ignored

# Run platform-specific tests
cargo test --target x86_64-apple-darwin -- --ignored
```

## Correctness Properties

Each property-based test validates specific requirements from the design document:

### Property 1: Log parsing preserves structure
Validates: Requirements 1.2
- For any valid JSON log entry, parsing extracts all required fields without data loss

### Property 2: Malformed entries don't halt processing
Validates: Requirements 1.4
- For any malformed JSON, parser skips entry and continues

### Property 3: Error and fault entries are captured
Validates: Requirements 1.3, 3.5
- For any log entry with Error or Fault type, entry is stored in buffer

### Property 18: Log stream restart on failure
Validates: Requirements 7.1
- For any subprocess failure scenario, collector restarts with exponential backoff
- **Note**: This property test is marked `#[ignore]` and `#[cfg(target_os = "macos")]` to avoid spawning real subprocesses during normal test runs

### Property 6: Rolling buffer maintains time-based expiration
Validates: Requirements 3.1
- For any event sequence, time-windowed queries return only events within window

### Property 7: Rolling buffer enforces capacity limits
Validates: Requirements 3.2
- For any buffer at max capacity, adding entry removes oldest and maintains size

### Property 8: Trigger activation on threshold breach
Validates: Requirements 3.3, 3.4
- For any threshold breach, trigger logic activates AI analysis

See `.kiro/specs/macos-system-observer/design.md` for complete property list.

## Test Utilities

### Subprocess Testing Strategy

The log collector includes comprehensive testing for subprocess management without spawning real processes during normal test runs:

**Unit Tests**: Fast, deterministic tests that verify state management and error handling without subprocess spawning:

```rust
#[test]
fn test_collector_state_consistency() {
    // Tests start/stop cycles with different predicates
    // Verifies state management without real subprocesses
}

#[test]
fn test_restart_backoff_behavior() {
    // Tests restart logic and state transitions
    // Uses mock scenarios instead of real failures
}
```

**Property-Based Tests (Ignored)**: Comprehensive tests that spawn real subprocesses but are ignored by default:

```rust
#[quickcheck]
#[cfg(target_os = "macos")]
#[ignore]
fn prop_collector_state_management_on_failure(_scenario: SubprocessFailureScenario) -> bool {
    // Spawns real log stream processes to test failure handling
    // Ignored to prevent subprocess spawning during CI/normal development
}
```

**Integration Tests**: Platform-specific tests for end-to-end validation:

```rust
#[test]
#[cfg(target_os = "macos")]
fn test_invalid_predicate_handling() {
    // Tests collector behavior with invalid log stream predicates
    // Verifies graceful handling of subprocess spawn failures
}
```

### Mock Backends

The AI analyzer includes comprehensive test utilities for mocking LLM backends:

```rust
pub struct MockBackend {
    expected_insight: AIInsight,
}

impl LLMBackend for MockBackend {
    fn analyze(&self, _context: &TriggerContext) -> Result<AIInsight, AnalysisError> {
        Ok(self.expected_insight.clone())
    }
}
```

### AI Analyzer Test Coverage

The `AIAnalyzer` module includes extensive test coverage:

- **AIInsight Creation**: Tests insight construction, confidence clamping, and tag management
- **Serialization**: Validates JSON round-trip serialization for all insight fields
- **Backend Integration**: Tests both placeholder and custom backend behavior
- **Analysis Methods**: Covers both `analyze()` and `summarize_activity()` methods
- **Confidence Validation**: Ensures confidence values are properly clamped to [0.0, 1.0]
- **Notification Formatting**: Tests summary generation for macOS notifications

```rust
#[tokio::test]
async fn test_analyzer_with_custom_backend() {
    let expected_insight = AIInsight::new(
        Severity::Critical,
        "Custom Analysis".to_string(),
        "Mock backend result".to_string(),
        vec!["Take action".to_string()],
        0.95,
    );

    let backend = Arc::new(MockBackend {
        expected_insight: expected_insight.clone(),
    });
    let analyzer = AIAnalyzer::with_backend(backend);

    let result = analyzer.analyze(&context).await;
    assert!(result.is_ok());
}
```

### Event Generators

```rust
pub fn generate_log_event() -> LogEvent {
    LogEvent {
        timestamp: Utc::now(),
        message_type: MessageType::Error,
        subsystem: "com.test".to_string(),
        process: "test".to_string(),
        message: "Test error".to_string(),
    }
}
```

### Time Manipulation

```rust
pub struct MockClock {
    current_time: DateTime<Utc>,
}

impl MockClock {
    pub fn advance(&mut self, duration: Duration) {
        self.current_time += duration;
    }
}
```

## Running Tests

```bash
# All tests (excludes ignored subprocess tests)
cargo test

# With output
cargo test -- --nocapture

# Specific test
cargo test test_log_parsing

# Property tests with more iterations
cargo test --release

# Integration tests only
cargo test --test '*'

# Platform-specific tests that spawn real subprocesses (requires macOS)
cargo test -- --ignored

# Run specific ignored property tests
cargo test prop_collector_state_management_on_failure -- --ignored

# Run all tests including subprocess tests (macOS only)
cargo test -- --include-ignored
```

### Test Categories

- **Default (`cargo test`)**: Unit tests, property tests with mocked data, deterministic state management tests
- **Ignored (`cargo test -- --ignored`)**: Tests that spawn real subprocesses, require macOS, or need special permissions
- **Platform-specific (`#[cfg(target_os = "macos")]`)**: Tests that use macOS-specific system tools
- **Property-based**: Tests with `#[quickcheck]` that use randomly generated inputs

## Coverage

Generate coverage reports:

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage
cargo tarpaulin --out Html
```

## Continuous Integration

Tests run automatically on:
- Every commit (unit + property tests)
- Pull requests (full test suite)
- Release builds (including integration tests)
