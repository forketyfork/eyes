# Subprocess Management

Eyes interfaces with macOS system tools through carefully managed subprocesses. This document describes the subprocess lifecycle, error handling, and testing strategies.

## Overview

The application spawns and manages two primary subprocesses:

- **`log stream`**: Streams macOS Unified Logs in JSON format
- **`powermetrics`**: Gathers system resource metrics (requires sudo)

## Subprocess Lifecycle

### Log Stream Collector

The `LogCollector` manages a `log stream` subprocess with the following lifecycle:

1. **Initialization**: Test subprocess spawn capability before starting background thread
2. **Spawn**: Create subprocess with predicate filter and JSON output
3. **Monitor**: Read stdout in non-blocking mode with timeout handling
4. **Parse**: Process JSON log entries line by line
5. **Restart**: Automatically restart on failure with exponential backoff
6. **Cleanup**: Graceful termination on shutdown

The collector includes comprehensive logging at each stage for improved observability:
- Startup progress tracking with predicate logging
- Subprocess spawn test results and error details
- Background thread lifecycle events
- Detailed error reporting for troubleshooting

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

### Metrics Collector

The `MetricsCollector` manages system resource monitoring with a multi-tier approach:

1. **Availability Test**: Check if powermetrics is available and accessible
2. **Spawn**: Attempt to spawn `sudo powermetrics` for detailed metrics
3. **Parse**: Handle plist format output from powermetrics
4. **Restart**: Automatic recovery with exponential backoff on failures
5. **Cleanup**: Graceful subprocess termination

```rust
// Primary: PowerMetrics with sudo
let child = Command::new("sudo")
    .args([
        "powermetrics",
        "--samplers", "cpu_power,gpu_power",
        "--format", "plist",
        "--sample-rate", &interval_ms.to_string(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;


```

### Restart Strategy

Subprocesses are automatically restarted on failure with exponential backoff:

- **Initial delay**: 1 second
- **Maximum delay**: 60 seconds
- **Backoff multiplier**: 2x on each consecutive failure
- **Failure threshold**: 5 consecutive failures before entering degraded mode
- **Degraded mode**: After 5 consecutive failures, wait 60 seconds before retrying
- **Responsive shutdown**: All sleep intervals use short polling to allow immediate shutdown

```rust
let mut restart_delay = Duration::from_secs(1);
let max_delay = Duration::from_secs(60);
let mut consecutive_failures = 0;
const MAX_CONSECUTIVE_FAILURES: u32 = 5;

// On failure:
restart_delay = std::cmp::min(restart_delay * 2, max_delay);

// Degraded mode with responsive shutdown:
let degraded_delay = Duration::from_secs(60);
let sleep_interval = Duration::from_millis(500);
let mut remaining = degraded_delay;
while remaining > Duration::ZERO && *running.lock().unwrap() {
    let sleep_time = std::cmp::min(remaining, sleep_interval);
    thread::sleep(sleep_time);
    remaining = remaining.saturating_sub(sleep_time);
}
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

- **Bounded buffers**: Limit memory usage for subprocess output with intelligent line-based parsing
- **Incremental processing**: Process complete lines as they arrive, buffering only incomplete data
- **Robust parsing**: Handle split JSON objects across read boundaries without data corruption
- **Cleanup on failure**: Properly terminate subprocesses to prevent resource leaks

See [Buffer Parsing](buffer-parsing.md) for detailed information on stream processing strategies.

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
- CPU power consumption (milliwatts)
- GPU power consumption (milliwatts)
- Memory pressure details (Normal/Warning/Critical)
- Thermal state information

**Graceful Degradation**: When sudo unavailable, enters degraded mode:
- Continues log monitoring without metrics collection
- Provides error messages indicating reduced functionality
- Allows system to continue operating with limited capabilities

### Notification Access

Automatically requested on first alert delivery:
- System will prompt user for notification permission
- Graceful degradation if permission denied

## Logging and Observability

### Startup Logging

The collectors provide detailed logging during startup for improved debugging:

**LogCollector startup sequence:**
```
INFO  Starting LogCollector with predicate: 'messageType == error OR messageType == fault'
DEBUG Testing log stream subprocess spawn capability
DEBUG Log stream subprocess test successful
DEBUG Spawning LogCollector background thread
INFO  LogCollector started successfully with predicate: 'messageType == error OR messageType == fault'
```

**Error scenarios:**
```
ERROR Failed to spawn log stream subprocess during startup test: log stream: No such file or directory
```

### Runtime Monitoring

Enable debug logging to monitor subprocess lifecycle:

```bash
# Via environment variable
RUST_LOG=debug cargo run

# Via CLI flag
cargo run -- --verbose
```

This shows:
- Subprocess spawn attempts and results
- Restart backoff timing and failure counts
- Thread lifecycle events
- Error details with context

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

**"sudo: no tty present"**: PowerMetrics requires interactive sudo
```bash
# Test powermetrics access
sudo powermetrics --help


```

**High CPU usage**: Check for rapid restart loops
```bash
# Monitor application logs (via environment variable)
RUST_LOG=debug cargo run

# Monitor application logs (via CLI flag)
cargo run -- --verbose
```

**Startup failures**: Check detailed startup logs
```bash
# Enable debug logging to see startup sequence
cargo run -- --verbose --config config.toml
```

Look for specific error patterns:
- `Failed to spawn log stream subprocess during startup test`: Permission or binary issues
- `LogCollector already running, skipping start`: Duplicate start attempts
- `Testing log stream subprocess spawn capability`: Startup validation process

### Debug Commands

```bash
# Test log stream manually
log stream --predicate "messageType == error" --style json

# Test powermetrics manually (requires sudo)
sudo powermetrics --samplers cpu_power,gpu_power --format plist --sample-rate 5000 -n 1

# Test powermetrics access
sudo powermetrics --help

# Check application logs (via environment variable)
RUST_LOG=debug cargo run 2>&1 | grep -E "(spawn|restart|failure|powermetrics)"

# Check application logs (via CLI flag)
cargo run -- --verbose 2>&1 | grep -E "(spawn|restart|failure|powermetrics)"
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