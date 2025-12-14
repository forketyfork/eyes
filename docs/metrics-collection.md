# Metrics Collection

The MetricsCollector provides comprehensive system resource monitoring for macOS through a multi-tier approach that prioritizes detailed metrics while ensuring graceful degradation.

## Architecture

### Primary Data Source: PowerMetrics

The collector primarily uses `sudo powermetrics` for detailed system metrics:

```bash
sudo powermetrics --samplers cpu_power,gpu_power --format plist --sample-rate 5000
```

**Advantages:**
- Precise CPU power consumption in milliwatts
- GPU power consumption when available
- Accurate memory pressure levels
- Thermal state information
- Hardware-level accuracy

**Requirements:**
- sudo privileges
- User interaction for password prompt
- macOS system integrity protection compatibility

### Fallback Operation (top + vm_stat)

When powermetrics is unavailable or fails to start, the collector switches to a fallback path:

**Characteristics:**
- Collects CPU usage estimates from `top`
- Retrieves memory pressure from `vm_stat` (periodically injected into events)
- GPU metrics are not available in fallback mode
- Continues emitting metrics events so trigger evaluation and AI analysis retain resource context
- After 5 consecutive failures, waits 60 seconds before retrying to avoid churn

## Data Formats

### PowerMetrics Plist Format

PowerMetrics outputs structured plist data:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>timestamp</key>
    <string>2024-12-09T18:30:45.123456Z</string>
    <key>cpu_power_mw</key>
    <real>1234.5</real>
    <key>gpu_power_mw</key>
    <real>567.8</real>
    <key>memory_pressure</key>
    <string>Warning</string>
</dict>
</plist>
```



## Error Handling

### Availability Testing

Before starting collection, the system tests tool availability:

1. **PowerMetrics Test**: `sudo powermetrics --help`
2. **Fallback Path**: Switch to `top`/`vm_stat` when powermetrics is unavailable (no GPU metrics in this mode)

### Subprocess Management

The collector implements robust subprocess lifecycle management:

```rust
// Exponential backoff parameters
let mut restart_delay = Duration::from_secs(1);
let max_delay = Duration::from_secs(60);
let mut consecutive_failures = 0;
const MAX_CONSECUTIVE_FAILURES: u32 = 5;

// Restart logic with backoff
restart_delay = std::cmp::min(restart_delay * 2, max_delay);
```

### Parsing Resilience

The buffer parsing handles multiple scenarios:

- **Incomplete Documents**: Buffers partial plist until complete
- **Mixed Content**: Processes valid entries, skips malformed ones
- **Plist Parsing**: Handles Apple's property list format
- **Memory Management**: Clears buffers after successful parsing

## Performance Considerations

### Sampling Intervals

Configurable sampling intervals balance accuracy with resource usage:

- **High Frequency** (1-2s): Real-time monitoring, higher CPU usage
- **Medium Frequency** (5s): Balanced monitoring (recommended)
- **Low Frequency** (10-30s): Background monitoring, minimal overhead

### Memory Usage

The collector maintains bounded memory usage:

- **Buffer Management**: Processes data incrementally
- **Event Streaming**: Sends events immediately after parsing
- **Resource Cleanup**: Proper subprocess termination prevents leaks

### CPU Impact

Minimal CPU overhead through efficient design:

- **Native Tools**: Leverages optimized system utilities
- **Selective Parsing**: Only processes complete documents
- **Thread Isolation**: Runs in dedicated background thread

## Security Considerations

### Privilege Escalation

PowerMetrics requires sudo access:

- **Interactive Prompts**: User must approve sudo access
- **Session Reuse**: Sudo sessions may be cached by system
- **Graceful Degradation**: Falls back when privileges unavailable

### Data Privacy

All metrics processing occurs locally:

- **No Network**: Metrics never transmitted externally
- **Local Processing**: All analysis happens on-device
- **Temporary Storage**: Events stored only in memory buffers

### System Integration

Minimal system impact through careful design:

- **Read-Only Access**: Only reads system metrics, never modifies
- **Standard Tools**: Uses documented macOS utilities
- **Resource Limits**: Bounded memory and CPU usage
