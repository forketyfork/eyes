# Subprocess Management

Eyes interfaces with macOS system tools through carefully managed subprocesses. This document describes the subprocess lifecycle, error handling, and testing strategies.

## Overview

The application spawns and manages two primary subprocesses:

- **`log stream`**: Streams macOS Unified Logs in JSON format
- **`powermetrics`**: Gathers system resource metrics (requires sudo)

## Subprocess Lifecycle

### Log Stream Collector

The `LogCollector` manages a `log stream` subprocess with the following lifecycle:

1. **Spawn**: Create subprocess with predicate filter and JSON output
2. **Monitor**: Read stdout in non-blocking mode with timeout handling
3. **Parse**: Process JSON log entries line by line
4. **Restart**: Automatically restart on failure with exponential backoff
5. **Cleanup**: Graceful termination on shutdown

```rust
// Spawn subprocess with non-blocking I/O
let mut child = Command::new("log")
    .args(&["stream", "--predicate", predicate, "--style", "json"])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;

// Set non-blocking mode for graceful shutdown
#[cfg(unix)]
{
    use std::os::unix::io::AsRawFd;
    let fd = stdout.as_raw_fd();
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}
```

### Restart Strategy

Subprocesses are automatically restarted on failure with exponential backoff:

- **Initial delay**: 1 second
- **Maximum delay**: 60 seconds
- **Backoff multiplier**: 2x on each consecutive failure
- **Failure threshold**: 5 consecutive failures before giving up

```rust
let mut restart_delay = Duration::from_secs(1);
let max_delay = Duration::from_secs(60);
let mut consecutive_failures = 0;
const MAX_CONSECUTIVE_FAILURES: u32 = 5;

// On failure:
restart_delay = std::cmp::min(restart_delay * 2, max_delay);
```

## Error Handling

### Subprocess Spawn Failures

- **Invalid predicate**: Log error and attempt restart
- **Missing binary**: Return `CollectorError::SubprocessSpawn`
- **Permission denied**: Return error with helpful message

### Runtime Failures

- **Subprocess exit**: Detect via `try_wait()` and restart
- **Broken pipe**: Handle gracefully and restart
- **Malformed output**: Skip invalid entries and continue

### Graceful Shutdown

- **Signal handling**: Respond to shutdown signals immediately
- **Non-blocking I/O**: Prevent hanging on subprocess reads
- **Resource cleanup**: Kill subprocess and wait for termination

## Testing Strategy

### Unit Tests (Fast)

Deterministic tests that verify state management without spawning real subprocesses:

```rust
#[test]
fn test_collector_state_consistency() {
    // Test start/stop cycles with different predicates
    // Verify state transitions without real subprocess spawning
}
```

### Property Tests (Ignored)

Comprehensive tests with real subprocesses, marked `#[ignore]` to prevent spawning during normal test runs:

```rust
#[quickcheck]
#[cfg(target_os = "macos")]
#[ignore]
fn prop_collector_state_management_on_failure(_scenario: SubprocessFailureScenario) -> bool {
    // Test with real log stream subprocess
    // Verify restart behavior under various failure conditions
}
```

### Integration Tests (Platform-Specific)

End-to-end tests that require macOS and appropriate permissions:

```rust
#[test]
#[cfg(target_os = "macos")]
fn test_invalid_predicate_handling() {
    // Test collector behavior with invalid log stream predicates
    // Verify graceful handling of subprocess failures
}
```

## Performance Considerations

### Non-Blocking I/O

All subprocess I/O uses non-blocking mode to prevent hanging:

- **Read timeout**: 10ms polling interval
- **Shutdown responsiveness**: Check running flag on each read attempt
- **Buffer management**: Process complete lines to avoid partial JSON

### Memory Management

- **Bounded buffers**: Limit memory usage for subprocess output
- **Line-by-line processing**: Avoid loading entire output into memory
- **Cleanup on failure**: Properly terminate subprocesses to prevent resource leaks

### CPU Usage

- **Efficient parsing**: Skip malformed entries without expensive error handling
- **Backoff strategy**: Prevent CPU spinning on repeated failures
- **Thread isolation**: Run collectors in separate threads to avoid blocking

## Permissions

### Log Stream Access

Requires **Full Disk Access** permission:
- System Preferences → Security & Privacy → Privacy → Full Disk Access
- Add the application binary to the allowed list

### PowerMetrics Access

Requires **sudo privileges** for enhanced metrics:
- CPU power consumption
- GPU power consumption
- Memory pressure details

### Notification Access

Automatically requested on first alert delivery:
- System will prompt user for notification permission
- Graceful degradation if permission denied

## Troubleshooting

### Common Issues

**"Operation not permitted"**: Missing Full Disk Access permission
```bash
# Check current permissions
log stream --predicate "messageType == error" --style json
```

**"Command not found"**: Missing system tools (rare on macOS)
```bash
# Verify tools are available
which log
which powermetrics
```

**High CPU usage**: Check for rapid restart loops
```bash
# Monitor application logs
RUST_LOG=debug cargo run
```

### Debug Commands

```bash
# Test log stream manually
log stream --predicate "messageType == error" --style json

# Test powermetrics manually (requires sudo)
sudo powermetrics --samplers cpu_power,gpu_power -n 1 --show-process-coalition

# Check application logs
RUST_LOG=debug cargo run 2>&1 | grep -E "(spawn|restart|failure)"
```

## Security Considerations

### Subprocess Isolation

- **Limited scope**: Only spawn known system binaries
- **Argument validation**: Validate predicates and parameters
- **Resource limits**: Prevent runaway subprocess resource usage

### Data Handling

- **Local processing**: All log data processed locally
- **No persistence**: Logs not stored permanently unless configured
- **Privacy**: System data never leaves machine when using Ollama backend

### Permission Model

- **Principle of least privilege**: Request only necessary permissions
- **Graceful degradation**: Continue operation with reduced functionality if permissions denied
- **User control**: Allow users to disable specific collectors