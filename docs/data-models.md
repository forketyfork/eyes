# Data Models

Eyes defines core data structures for representing system events, metrics, and analysis results. All types are designed for efficient serialization and thread-safe passing between components.

## Event Types

### LogEvent

Represents a single entry from the macOS Unified Log System, captured via `log stream --style json`.

```rust
pub struct LogEvent {
    pub timestamp: Timestamp,        // UTC timestamp
    pub message_type: MessageType,   // error, fault, info, debug
    pub subsystem: String,           // e.g., "com.apple.WindowServer"
    pub category: String,            // e.g., "rendering"
    pub process: String,             // Process name
    pub process_id: u32,             // PID
    pub message: String,             // Log message content
}
```

**Key Properties:**
- Fully serializable to/from JSON via serde
- Clone-able for efficient passing between threads
- Includes all metadata needed for AI analysis context

**Parsing from macOS JSON:**

The `from_json` method parses log entries directly from `log stream --style json` output:

```rust
let json = r#"{
    "timestamp": "2024-12-09 10:30:45.123456-0800",
    "messageType": "Error",
    "subsystem": "com.apple.Safari",
    "category": "WebProcess",
    "process": "Safari",
    "processID": 1234,
    "message": "Failed to load resource"
}"#;

let event = LogEvent::from_json(json)?;
```

The parser handles:
- macOS timestamp format: `YYYY-MM-DD HH:MM:SS.ffffff-ZZZZ`
- Field name mapping: `messageType` → `message_type`, `processID` → `process_id`
- Case-insensitive message type parsing
- Conversion to UTC timestamps

### MetricsEvent

Point-in-time snapshot of system resource usage, typically from `powermetrics`.

```rust
pub struct MetricsEvent {
    pub timestamp: Timestamp,
    pub cpu_power_mw: f64,           // CPU power in milliwatts
    pub cpu_usage_percent: f64,      // CPU usage percentage (0-100)
    pub gpu_power_mw: Option<f64>,   // GPU power in milliwatts (None if unavailable)
    pub gpu_usage_percent: Option<f64>, // GPU usage percentage (None if unavailable)
    pub memory_pressure: MemoryPressure,
    pub memory_used_mb: f64,         // Memory usage in megabytes
    pub energy_impact: f64,          // Total energy impact in milliwatts
}
```

**Key Properties:**
- Captures both power consumption (milliwatts) and usage percentages
- GPU metrics are optional to handle systems without discrete GPUs
- Energy impact provides comprehensive power consumption insight
- Memory usage in megabytes for precise tracking

### DiskEvent

Disk I/O activity snapshot from `iostat`, with optional filesystem context from `fs_usage` when available.

```rust
pub struct DiskEvent {
    pub timestamp: Timestamp,
    pub disk_name: String,           // Device name or "fs_usage" for filesystem events
    pub read_kb_per_sec: f64,        // Read throughput in KB/s
    pub write_kb_per_sec: f64,       // Write throughput in KB/s
    pub read_ops_per_sec: f64,       // Read operations per second
    pub write_ops_per_sec: f64,      // Write operations per second
    pub filesystem_path: Option<String>, // Path from fs_usage when available
}
```

**Key Properties:**
- Captures both throughput (KB/s) and operation rates (ops/s)
- Device-specific monitoring for multi-disk systems
- Best-effort filesystem context when `fs_usage` is available (populates `filesystem_path`)
- Real-time I/O performance tracking for both iostat and fs_usage streams

**Parsing from iostat:**

The `from_iostat_line` method parses disk metrics from `iostat` output:

```rust
let line = "disk0       123.45   67.89    12.3     6.7";
let event = DiskEvent::from_iostat_line(line)?;
assert_eq!(event.disk_name, "disk0");
```

`fs_usage` lines are parsed separately in the disk collector; those events set `disk_name` to `"fs_usage"` and fill `filesystem_path` when a path is present.

## Enumerations

### MessageType

Log severity levels from the Unified Log System:

```rust
pub enum MessageType {
    Error,   // Indicates a problem
    Fault,   // Serious issue
    Info,    // Informational
    Debug,   // Debug-level
}
```

Serializes to lowercase strings: `"error"`, `"fault"`, `"info"`, `"debug"`.

### MemoryPressure

macOS memory management pressure levels:

```rust
pub enum MemoryPressure {
    Normal,   // Healthy memory conditions
    Warning,  // System under pressure
    Critical, // May start killing processes
}
```

**Ordering:** `Normal < Warning < Critical` for threshold comparisons.

Serializes to PascalCase: `"Normal"`, `"Warning"`, `"Critical"`.

### Severity

AI-generated insight severity for alerting:

```rust
pub enum Severity {
    Info,     // No action required
    Warning,  // May need attention
    Critical, // Immediate attention needed
}
```

**Ordering:** `Info < Warning < Critical` for alert prioritization.

Serializes to lowercase: `"info"`, `"warning"`, `"critical"`.

## Type Aliases

### Timestamp

```rust
pub type Timestamp = DateTime<Utc>;
```

Consistent UTC timestamp handling across the application using `chrono`.

## Design Decisions

### Why Clone Instead of Arc?

Events are cloned when passed between threads rather than using `Arc<T>`:
- Events are small (typically < 1KB)
- Cloning avoids lifetime complexity
- Enables independent mutation in different pipeline stages
- Simplifies testing with owned values

### Serialization Format

All types use serde with JSON as the primary format:
- Matches `log stream --style json` output
- Human-readable for debugging
- Compatible with AI prompt formatting
- Easy integration with external tools

### Optional Fields

Only `gpu_usage` is optional because:
- Not all Macs have discrete GPUs
- Allows graceful degradation on unsupported hardware
- Other metrics are always available from macOS APIs

## Testing

Each type includes comprehensive unit tests:
- Serialization round-trip tests
- Enum ordering verification
- JSON format validation

See `src/events.rs` for the complete test suite.
