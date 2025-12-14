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
- **Comprehensive Logging**: Detailed startup and runtime logging for debugging and monitoring

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
- **Startup Validation**: Tests subprocess spawn capability before starting background thread
- **Observability**: Comprehensive logging at all lifecycle stages for debugging and monitoring

## Metrics Collector

The `MetricsCollector` interfaces with macOS system resource monitoring tools to gather real-time performance data.

### Features

- **PowerMetrics Integration**: Primary data source using `sudo powermetrics` for detailed system metrics
- **Fallback Collection**: Falls back to `top` + `vm_stat` when powermetrics is unavailable or fails to start
- **Plist Format Support**: Parses plist output format from powermetrics, and plain-text output from `top`/`vm_stat`
- **Automatic Restart**: Recovers from subprocess failures with exponential backoff
- **Thread Safety**: Runs in dedicated background thread with channel communication
- **Configurable Sampling**: User-defined collection intervals

### Usage

```rust
use std::sync::mpsc;
use std::time::Duration;
use eyes::collectors::MetricsCollector;

let (tx, rx) = mpsc::channel();
let mut collector = MetricsCollector::new(Duration::from_secs(5), tx);

// Start collecting in background thread
collector.start()?;

// Receive parsed events
while let Ok(event) = rx.recv() {
    println!("Metrics event: CPU: {:.1}mW, GPU: {:?}mW, Memory: {:?}", 
             event.cpu_power_mw, event.gpu_power_mw, event.memory_pressure);
}

// Stop gracefully
collector.stop()?;
```

### Data Sources

#### Primary: PowerMetrics
Uses `sudo powermetrics` for comprehensive system metrics:
- CPU power consumption (milliwatts)
- GPU power consumption (milliwatts) 
- Memory pressure levels (Normal/Warning/Critical)
- Thermal state information

```bash
sudo powermetrics --samplers cpu_power,gpu_power --format plist --sample-rate 5000
```

#### Fallback Mode
When powermetrics is unavailable or cannot start:
- Switches to `top` + `vm_stat` for estimated CPU usage and memory pressure
- GPU metrics are not available in fallback mode
- Continues emitting metrics events so triggers and AI analysis still receive resource context

### Error Recovery

The collector implements comprehensive error recovery:

1. **Availability Testing**: Tests powermetrics availability before starting
2. **Fallback**: Switches to `top`/`vm_stat` when powermetrics fails to start
3. **Subprocess Restart**: Exponential backoff restart strategy (1s to 60s)
4. **Plist Parsing**: Handles plist format parsing gracefully
5. **Failure Limits**: After 5 consecutive failures, waits 60 seconds before retrying to reduce churn
6. **Resource Cleanup**: Proper subprocess termination on shutdown

### Implementation Details

- **Thread Model**: Single background thread per collector instance
- **Plist Parsing**: Supports plist format from powermetrics
- **Buffer Management**: Handles partial reads and incomplete documents
- **Privilege Handling**: Graceful degradation when sudo unavailable
- **Memory Safety**: Thread-safe shutdown signaling with `Arc<Mutex<bool>>`
- **Process Management**: Proper child process lifecycle management

## Disk Collector

The `DiskCollector` gathers disk I/O metrics via `iostat` and best-effort filesystem activity from `fs_usage` when sudo is available.

### Features

- **iostat Integration**: Captures read/write throughput and operation rates without elevated privileges
- **fs_usage (Optional)**: Adds filesystem path context when sudo access is available; automatically skipped otherwise
- **Adaptive Sampling**: Shares the metrics sampling interval and slows down under resource pressure
- **Automatic Restart**: Restarts on failure with exponential backoff; waits 60 seconds after repeated failures
- **Thread Safety**: Dedicated background thread with channel communication

### Usage

```rust
use std::sync::mpsc;
use std::time::Duration;
use eyes::collectors::DiskCollector;

let (tx, rx) = mpsc::channel();
let mut collector = DiskCollector::new(Duration::from_secs(5), tx);
collector.start()?;

for event in rx {
    println!(
        "Disk {} read {:.1} KB/s write {:.1} KB/s ({:?})",
        event.disk_name, event.read_kb_per_sec, event.filesystem_path
    );
}
```

### Error Handling

- **Tool Availability**: Logs and continues with iostat-only monitoring when `fs_usage` cannot be run
- **Parse Errors**: Skips malformed lines without halting the collector
- **Failure Backoff**: After 5 consecutive failures, pauses for 60 seconds before retrying

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
