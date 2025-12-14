# Error Handling and Resilience

Eyes implements comprehensive error handling and resilience patterns to ensure reliable operation even when individual components fail. This document describes the error handling strategies used throughout the system.

## Overview

The application uses a multi-layered approach to error handling:

1. **Component-Level Resilience**: Each component handles its own failure modes
2. **Retry Mechanisms**: Automatic retry with exponential backoff for transient failures
3. **Graceful Degradation**: Continue operation with reduced functionality when components fail
4. **Circuit Breaker Patterns**: Prevent cascading failures through intelligent failure detection

## AI Analysis Retry Queue

The AI analyzer includes a sophisticated retry queue system for handling backend failures:

### Architecture

```rust
struct RetryEntry {
    context: TriggerContext,
    attempt_count: u32,
    next_retry_time: Instant,
}

pub struct AIAnalyzer {
    retry_queue: Arc<Mutex<VecDeque<RetryEntry>>>,
    max_retry_attempts: u32,
    max_queue_size: usize,
    base_retry_delay: Duration,
}
```

### Retry Strategy

- **Exponential Backoff**: Delays increase as 1s, 2s, 4s, 8s for successive failures
- **Maximum Attempts**: Default limit of 3 retry attempts per request
- **Queue Bounds**: Maximum 100 queued entries to prevent memory exhaustion
- **Overflow Handling**: Oldest entries are dropped when queue is full

### Usage Patterns

```rust
// Automatic retry on failure
match analyzer.analyze(&context).await {
    Ok(insight) => { /* Process successful analysis */ }
    Err(e) => { /* Request automatically queued for retry */ }
}

// Process retry queue periodically
let retry_results = analyzer.process_retry_queue().await;
for result in retry_results {
    match result {
        Ok(insight) => { /* Retry succeeded */ }
        Err(e) => { /* Max attempts reached */ }
    }
}

// Monitor queue status
let queue_size = analyzer.retry_queue_size();
if queue_size > 50 {
    warn!("High retry queue size: {}", queue_size);
}
```

## Collector Resilience

### Subprocess Management

Both log and metrics collectors implement robust subprocess management:

- **Startup Validation**: Test subprocess spawn capability before starting background threads
- **Automatic Restart**: Restart failed subprocesses with exponential backoff
- **Degraded Mode**: Enter degraded mode after consecutive failures (5 attempts)
- **Responsive Shutdown**: Non-blocking I/O prevents hanging during shutdown

### Failure Scenarios

**Log Collector:**
- Invalid predicates: Log error and restart with exponential backoff
- Permission denied: Return error with helpful message about Full Disk Access
- Subprocess crash: Detect via `try_wait()` and restart automatically

**Metrics Collector:**
- PowerMetrics unavailable: Switch to fallback metrics via `top`/`vm_stat` (no GPU metrics)
- Sudo timeout: Graceful degradation with informative error messages
- Parse failures: Skip malformed entries and continue processing

### Buffer Parsing Resilience

- **Partial Reads**: Handle data split across read boundaries
- **Malformed Entries**: Skip invalid JSON without halting processing
- **Memory Protection**: Bounded buffers prevent memory exhaustion
- **UTF-8 Safety**: Proper handling of invalid UTF-8 sequences

## Alert System Resilience

### Rate Limiting and Queueing

The alert manager implements intelligent queueing for rate-limited scenarios:

- **Immediate Delivery**: Send alerts immediately when rate limits allow
- **Queue on Limit**: Queue critical alerts when rate limited
- **Autonomous Processing**: Background processing of queued alerts
- **Overflow Protection**: Drop oldest alerts when queue is full

### Notification Failures

- **osascript Errors**: Log failures but continue system operation
- **Permission Issues**: Graceful handling with informative error messages
- **System Unavailable**: Continue monitoring even if notifications fail

## Configuration Resilience

### File Loading

- **Missing Files**: Fall back to defaults with warning messages
- **Invalid Syntax**: Log errors and use defaults for invalid sections
- **Partial Configuration**: Merge user config with defaults for missing fields

### Validation

- **Schema Validation**: Verify configuration structure and types
- **Range Checking**: Ensure numeric values are within reasonable bounds
- **Path Validation**: Check file paths and permissions where applicable

## Network Resilience

### HTTP Client Configuration

```rust
let client = Client::builder()
    .timeout(Duration::from_secs(60))
    .no_proxy()
    .build()?;
```

### Backend Communication

- **Timeout Handling**: 60-second timeout for LLM requests
- **Connection Errors**: Automatic retry through the retry queue system
- **Authentication Failures**: Clear error messages for invalid API keys
- **Rate Limiting**: Respect backend rate limits and retry appropriately

## Monitoring and Observability

### Self-Monitoring Integration

All error handling integrates with the self-monitoring system:

- **Failure Rates**: Track success/failure rates for all operations
- **Retry Metrics**: Monitor retry queue size and processing rates
- **Performance Impact**: Measure latency impact of retry operations
- **Alert Delivery**: Track notification success rates

### Logging Strategy

```rust
// Error context with structured logging
error!("AI analysis failed after {:?}: {}", duration, e);
warn!("Retry queue is full, dropping oldest entry");
info!("Queued analysis for retry, queue size: {}", queue_len);
debug!("Retrying analysis attempt {} for trigger: {}", attempt, trigger);
```

## Testing Error Scenarios

### Unit Tests

```rust
#[test]
fn test_retry_queue_overflow() {
    // Test queue behavior when full
}

#[test]
fn test_exponential_backoff_timing() {
    // Verify retry delay calculations
}
```

### Property-Based Tests

```rust
#[quickcheck]
fn prop_retry_queue_bounds(entries: Vec<TriggerContext>) -> bool {
    // Verify queue never exceeds maximum size
}
```

### Integration Tests

```rust
#[test]
#[ignore = "Requires network failure simulation"]
fn test_backend_failure_recovery() {
    // Test end-to-end retry behavior
}
```

## Best Practices

### Error Propagation

- Use `Result<T, E>` for recoverable errors
- Use `anyhow` for application-level error context
- Use `thiserror` for custom error types with structured information

### Logging Guidelines

- **Error Level**: Unrecoverable failures that require attention
- **Warn Level**: Recoverable failures and degraded operation
- **Info Level**: Normal operation state changes
- **Debug Level**: Detailed troubleshooting information

### Resource Management

- Always clean up resources in error paths
- Use RAII patterns with `Drop` implementations
- Implement proper shutdown sequences for background threads

### User Experience

- Provide clear error messages with actionable guidance
- Continue operation when possible rather than failing completely
- Use progressive degradation to maintain core functionality
