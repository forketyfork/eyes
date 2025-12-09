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

### MetricsEvent

Point-in-time snapshot of system resource usage, typically from `powermetrics`.

```rust
pub struct MetricsEvent {
    pub timestamp: Timestamp,
    pub cpu_usage: f64,              // Percentage (0-100)
    pub memory_pressure: MemoryPressure,
    pub memory_used_gb: f64,
    pub gpu_usage: Option<f64>,      // None if unavailable
    pub energy_impact: f64,          // Arbitrary units
}
```

**Key Properties:**
- Captures both absolute values (memory_used_gb) and relative metrics (cpu_usage)
- GPU usage is optional to handle systems without discrete GPUs
- Energy impact provides macOS-specific power consumption insight

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
