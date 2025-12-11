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

### Fallback Data Source: System Tools

When powermetrics is unavailable, the collector automatically falls back to alternative tools:

```bash
# Memory pressure estimation via vm_stat with robust parsing
FREE_PAGES=$(vm_stat | grep 'Pages free:' | awk '{print $3}' | tr -d '.')
if [ "$FREE_PAGES" -lt 100000 ]; then
    PRESSURE="Critical"
elif [ "$FREE_PAGES" -lt 500000 ]; then
    PRESSURE="Warning"
else
    PRESSURE="Normal"
fi

# Synthetic JSON output for compatibility
echo "{\"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%S.%6NZ)\", \"cpu_power_mw\": 0.0, \"gpu_power_mw\": null, \"memory_pressure\": \"$PRESSURE\"}"
```

**Characteristics:**
- No sudo required
- Limited accuracy (synthetic CPU/GPU data)
- Robust memory pressure estimation with proper shell variable handling
- Maintains API compatibility with improved parsing reliability

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

### Fallback JSON Format

Fallback tools output simplified JSON:

```json
{
    "timestamp": "2024-12-09T18:30:45.123456Z",
    "cpu_power_mw": 0.0,
    "gpu_power_mw": null,
    "memory_pressure": "Normal"
}
```

## Error Handling

### Availability Testing

Before starting collection, the system tests tool availability:

1. **PowerMetrics Test**: `powermetrics --help`
2. **Fallback Test**: `vm_stat` and `top -l 1 -n 0`
3. **Graceful Degradation**: Automatic fallback selection

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

- **Incomplete Documents**: Buffers partial plist/JSON until complete
- **Mixed Content**: Processes valid entries, skips malformed ones
- **Format Detection**: Automatically detects plist vs JSON format
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