# Data Collectors

Eyes uses specialized collectors to interface with macOS system tools and gather real-time data.

## Log Collector

The `LogCollector` interfaces with macOS Unified Logging System via the `log stream` command.

### Features

- **Subprocess Management**: Spawns and monitors `log stream` as a child process
- **JSON Parsing**: Parses structured log output into `LogEvent` structures
- **Automatic Restart**: Recovers from subprocess failures with exponential backoff
- **Graceful Error Handling**: Continues processing despite malformed log entries
- **Thread Safety**: Runs in dedicated background thread with channel communication

### Usage

```rust
use std::sync::mpsc;
use eyes::collectors::LogCollector;

let (tx, rx) = mpsc::channel();
let mut collector = LogCollector::new(
    "messageType == error OR messageType == fault".to_string(),
    tx
);

// Start collecting in background thread
collector.start()?;

// Receive parsed events
while let Ok(event) = rx.recv() {
    println!("Log event: {:?}", event);
}

// Stop gracefully
collector.stop()?;
```

### Predicate Filtering

Uses Apple's predicate syntax for efficient server-side filtering:

```rust
// Error and fault messages only
"messageType == error OR messageType == fault"

// Specific subsystem
"subsystem == 'com.apple.Safari'"

// Process-specific logs
"process == 'Safari' AND messageType == error"

// Text search in messages
"message CONTAINS 'memory' AND messageType == error"
```

### Error Recovery

The collector implements robust error recovery:

1. **Subprocess Failures**: Automatically restarts `log stream` with exponential backoff
2. **Exit Status Detection**: Distinguishes between normal shutdown and subprocess failures by checking exit status
3. **Malformed JSON**: Logs parsing errors but continues processing subsequent entries
4. **Channel Closure**: Detects receiver shutdown and terminates gracefully
5. **Resource Limits**: Enforces maximum consecutive failure threshold (5 failures)
6. **Non-blocking Shutdown**: Uses non-blocking I/O to prevent hanging during graceful shutdown
7. **Partial Line Handling**: Buffers incomplete lines across read operations for reliable parsing

### Implementation Details

- **Thread Model**: Single background thread per collector instance
- **Non-blocking I/O**: Uses non-blocking reads with manual buffering for responsive shutdown
- **Restart Logic**: Exponential backoff from 1s to 60s maximum delay with intelligent failure detection
- **Exit Status Monitoring**: Checks subprocess exit status to distinguish failures from normal termination
- **Memory Safety**: Uses `Arc<Mutex<bool>>` for thread-safe shutdown signaling
- **Process Cleanup**: Ensures child processes are properly terminated on shutdown
- **Buffer Management**: Manual line buffering with 4KB read chunks for efficient processing

## Metrics Collector

*Implementation pending - see task 6 in implementation plan*

The `MetricsCollector` will interface with `powermetrics` to gather:
- CPU power consumption
- GPU power usage  
- Memory pressure levels
- Thermal state information

### Planned Features

- **Plist Parsing**: Parse `powermetrics` property list output
- **Privilege Handling**: Graceful degradation when sudo unavailable
- **Sampling Control**: Configurable collection intervals
- **Fallback Sources**: Alternative data sources when `powermetrics` unavailable

## Testing Strategy

Both collectors use comprehensive testing approaches:

### Unit Tests
- Creation and lifecycle management
- Start/stop behavior
- Error condition handling
- Mock subprocess interaction

### Property-Based Tests
- **Malformed Input Handling**: Verifies graceful handling of invalid JSON/plist data
- **Error/Fault Capture**: Ensures all relevant log entries are captured
- **Restart Behavior**: Validates automatic recovery from subprocess failures
- **Rapid Cycling**: Tests collector stability under frequent start/stop operations

### Integration Tests
- Real subprocess interaction (marked with `#[ignore]` for CI)
- macOS permission requirements
- End-to-end data flow validation

See [Testing](testing.md) for detailed testing guidelines.