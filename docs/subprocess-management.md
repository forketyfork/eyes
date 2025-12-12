# Subprocess Management

Eyes interfaces with macOS system tools through carefully managed subprocesses. This document describes the subprocess lifecycle, error handling, and testing strategies.

## Overview

The application spawns and manages two primary subprocesses:

- **`log stream`**: Streams macOS Unified Logs in JSON format
- **`powermetrics`**: Gathers system resource metrics (requires sudo)
- **Fallback tools**: `vm_stat`, `top` for graceful degradation

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

### Metrics Collector

The `MetricsCollector` manages system resource monitoring with a multi-tier approach:

1. **Availability Test**: Check if powermetrics is available and accessible
2. **Primary Spawn**: Attempt to spawn `sudo powermetrics` for detailed metrics
3. **Fallback Spawn**: Use `vm_stat` and shell scripts if powermetrics unavailable
4. **Dual Parsing**: Handle both plist (powermetrics) and JSON (fallback) formats
5. **Restart**: Automatic recovery with exponential backoff on failures
6. **Cleanup**: Graceful subprocess termination

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

// Fallback: Shell script with vm_stat
let script = format!(
    r#"
    while true; do
        # Get memory pressure from vm_stat
        FREE_PAGES=$(vm_stat | grep 'Pages free:' | awk '{{print $3}}' | tr -d '.')
        if [ "$FREE_PAGES" -lt 100000 ]; then
            PRESSURE="Critical"
        elif [ "$FREE_PAGES" -lt 500000 ]; then
            PRESSURE="Warning"
        else
            PRESSURE="Normal"
        fi
        
        # Output valid JSON
        echo "{\"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%S.%6NZ)\", \"cpu_power_mw\": 0.0, \"gpu_power_mw\": null, \"memory_pressure\": \"$PRESSURE\"}"
        sleep {}
    done
    "#,
    interval_secs
);

let child = Command::new("sh")
    .args(["-c", &script])
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

**Graceful Degradation**: When sudo unavailable, automatically falls back to:
- `vm_stat` for memory pressure estimation with robust shell script parsing
- Synthetic CPU power data (0.0 mW)
- Basic system monitoring via shell scripts with proper variable handling

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
which vm_stat
```

**"sudo: no tty present"**: PowerMetrics requires interactive sudo
```bash
# Test powermetrics access
sudo powermetrics --help

# Check fallback availability
vm_stat
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
sudo powermetrics --samplers cpu_power,gpu_power --format plist --sample-rate 5000 -n 1

# Test fallback monitoring
vm_stat

# Test the improved fallback script logic
FREE_PAGES=$(vm_stat | grep 'Pages free:' | awk '{print $3}' | tr -d '.')
if [ "$FREE_PAGES" -lt 100000 ]; then
    PRESSURE="Critical"
elif [ "$FREE_PAGES" -lt 500000 ]; then
    PRESSURE="Warning"
else
    PRESSURE="Normal"
fi
echo "{\"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%S.%6NZ)\", \"cpu_power_mw\": 0.0, \"gpu_power_mw\": null, \"memory_pressure\": \"$PRESSURE\"}"

# Check application logs
RUST_LOG=debug cargo run 2>&1 | grep -E "(spawn|restart|failure|powermetrics|fallback)"
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