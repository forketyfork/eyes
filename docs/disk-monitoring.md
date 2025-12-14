# Disk Monitoring

Eyes monitors disk I/O activity and filesystem events using native macOS tools to detect performance issues, excessive disk usage, and storage-related problems.

## Overview

The disk monitoring system provides insights into:

- **Disk I/O Performance**: Read/write throughput and operation rates
- **Storage Activity**: Real-time disk usage patterns
- **Performance Bottlenecks**: Identification of disk-bound processes
- **Filesystem Events**: File system operations and access patterns

## Architecture

### DiskCollector

The `DiskCollector` manages disk monitoring through native macOS tools:

```rust
use eyes::collectors::DiskCollector;
use std::sync::mpsc;
use std::time::Duration;

let (tx, rx) = mpsc::channel();
let mut collector = DiskCollector::new(Duration::from_secs(5), tx);

// Start monitoring
collector.start()?;

// Process disk events
for event in rx {
    println!(
        "Disk activity: {} - Read: {:.1} KB/s, Write: {:.1} KB/s ({:?})",
        event.disk_name, event.read_kb_per_sec, event.write_kb_per_sec, event.filesystem_path
    );
}
```

### Key Features

- **Adaptive Sampling**: Automatically adjusts monitoring frequency based on system resource pressure
- **Tool Availability Testing**: Validates disk monitoring tools before starting collection
- **Best-Effort Filesystem Context**: Uses `fs_usage` when sudo access is available; falls back to iostat-only when it is not
- **Automatic Restart**: Recovers from subprocess failures with exponential backoff
- **Self-Monitoring Integration**: Tracks performance and adjusts behavior under resource pressure

## Data Collection

### Primary Tool: iostat

Uses `iostat` for disk I/O statistics without requiring elevated privileges:

```bash
iostat -d -c 1 5  # Disk stats, continuous, 5-second intervals
```

**Output Format:**
```
disk0       123.45   67.89    12.3     6.7
```

Where columns represent:
- Device name (e.g., "disk0")
- Read throughput (KB/s)
- Write throughput (KB/s)
- Read operations per second
- Write operations per second

### Secondary Tool: fs_usage (Optional)

When available with sudo privileges, `fs_usage` provides detailed filesystem event tracking:

```bash
sudo fs_usage -f filesys  # Filesystem operations only
```

**Note**: `fs_usage` requires sudo privileges and may not be available in all environments. The collector logs a warning and continues with iostat-only monitoring when sudo access is unavailable.

## DiskEvent Structure

Disk activity is represented by the `DiskEvent` structure:

```rust
pub struct DiskEvent {
    pub timestamp: Timestamp,
    pub disk_name: String,        // Device name (e.g., "disk0") or "fs_usage" for filesystem events
    pub read_kb_per_sec: f64,     // Read throughput in KB/s
    pub write_kb_per_sec: f64,    // Write throughput in KB/s
    pub read_ops_per_sec: f64,    // Read operations per second
    pub write_ops_per_sec: f64,   // Write operations per second
    pub filesystem_path: Option<String>, // Path parsed from fs_usage when available
}
```

### Parsing

Events are parsed from `iostat` output using robust line-by-line processing:

```rust
// Parse iostat line: "disk0  123.45  67.89  12.3  6.7"
let event = DiskEvent::from_iostat_line(line)?;
```

## Integration with AI Analysis

Disk events are integrated into the AI analysis pipeline for intelligent diagnostics:

### Trigger Rules

Future trigger rules will detect disk-related issues:
- **High I/O Activity**: Excessive read/write operations
- **Performance Degradation**: Sudden drops in throughput
- **Storage Pressure**: High disk utilization patterns

### AI Insights

The AI analyzer can identify disk-related problems:
- Applications causing excessive disk I/O
- Storage bottlenecks affecting system performance
- Recommendations for optimizing disk usage

## Configuration

- The disk collector uses the same sampling interval as `metrics.interval_seconds`.
- There is no dedicated disk configuration block yet; failures starting disk monitoring are logged and the application continues running.
- `fs_usage` is enabled on a best-effort basis and skipped automatically when sudo access is unavailable.

## Adaptive Sampling

The disk collector implements adaptive sampling to reduce resource usage under pressure:

### Pressure Detection

When the self-monitoring system detects resource pressure:
- **Increase Interval**: Sampling frequency is reduced (1.5x multiplier)
- **Maximum Interval**: Capped at 60 seconds to maintain responsiveness
- **Gradual Recovery**: Frequency gradually returns to baseline when pressure is relieved

### Benefits

- **Resource Efficiency**: Reduces CPU and memory usage during high system load
- **Maintained Monitoring**: Continues disk monitoring even under pressure
- **Automatic Recovery**: Returns to normal sampling when conditions improve

## Error Handling

### Tool Availability

The collector tests tool availability before starting:

```rust
// Test iostat availability
let result = Command::new("iostat").args(["-d", "-c", "1"]).output();

// Test fs_usage availability (with sudo)
let result = Command::new("sudo").args(["-n", "fs_usage", "-h"]).output();
```

### Failure Recovery

- **Subprocess Failures**: Automatic restart with exponential backoff
- **Parse Errors**: Skip malformed entries and continue processing
- **Tool Unavailability**: Graceful degradation with informative error messages
- **Degraded Mode**: After 5 consecutive failures, the collector waits 60 seconds before retrying to reduce churn

### Responsive Shutdown

- **Non-blocking I/O**: Prevents hanging during shutdown
- **Signal Handling**: Responds immediately to stop signals
- **Resource Cleanup**: Properly terminates subprocesses and cleans up resources

## Performance Considerations

### Memory Usage

- **Bounded Buffers**: Limits memory usage for subprocess output
- **Incremental Processing**: Processes complete lines as they arrive
- **Efficient Parsing**: Minimal memory allocation during parsing

### CPU Usage

- **Efficient Subprocess Management**: Minimal overhead for process monitoring
- **Optimized Parsing**: Fast line-by-line processing without expensive operations
- **Backoff Strategy**: Prevents CPU spinning on repeated failures

### I/O Efficiency

- **Non-blocking Reads**: Uses non-blocking I/O to prevent thread blocking
- **Buffer Management**: Intelligent buffering to handle partial reads
- **Minimal System Impact**: Lightweight monitoring with minimal system overhead

## Testing

### Unit Tests

```bash
# Test disk collector creation and basic functionality
cargo test disk_collector

# Test iostat parsing
cargo test disk_event_from_iostat_line
```

### Integration Tests

```bash
# Test with actual macOS tools (requires macOS)
cargo test --ignored test_disk_collector_start_stop

# Test tool availability detection
cargo test test_disk_tools_availability_check
```

### Property-Based Tests

Future property-based tests will validate:
- Parsing robustness across various iostat output formats
- Buffer management under different data arrival patterns
- Error handling consistency across failure scenarios

## Troubleshooting

### Common Issues

**"iostat: command not found"**: 
- iostat should be available on all macOS systems
- Verify system integrity and PATH configuration

**"fs_usage requires password"**:
- fs_usage requires sudo privileges
- This is expected and the collector will continue with iostat only

**High CPU usage**:
- Check for rapid restart loops in logs
- Verify disk monitoring tools are functioning correctly

### Debug Logging

Enable debug logging to monitor disk collection:

```bash
# Via environment variable
RUST_LOG=debug cargo run

# Via CLI flag
cargo run -- --verbose
```

Shows:
- Tool availability test results
- Subprocess spawn attempts and results
- Parsing success/failure details
- Adaptive sampling adjustments

## Future Enhancements

### Advanced Metrics

Potential future enhancements:
- **Disk Space Monitoring**: Track available storage space
- **File System Events**: Detailed file operation tracking
- **Performance Baselines**: Historical performance comparison
- **Predictive Analysis**: AI-powered disk failure prediction

### Integration Improvements

- **Trigger Rules**: Disk-specific trigger rules for AI analysis
- **Alert Templates**: Disk-related notification templates
- **Configuration**: More granular disk monitoring configuration options
- **Visualization**: Real-time disk activity visualization (if GUI is added)

## Security Considerations

### Permissions

- **iostat**: No special permissions required
- **fs_usage**: Requires sudo privileges (optional)
- **Data Privacy**: All disk monitoring data stays local

### Resource Limits

- **Subprocess Isolation**: Disk monitoring tools run in isolated subprocesses
- **Resource Bounds**: Memory and CPU usage are bounded and monitored
- **Graceful Degradation**: System continues operation if disk monitoring fails
