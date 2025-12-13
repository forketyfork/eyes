# Design Document

## Overview

The macOS System Observer is a multi-threaded Rust application that monitors system health through three primary data streams: the Unified Log System, powermetrics for resource consumption, and filesystem events. The application uses a producer-consumer architecture where data collectors feed into a central analysis engine that applies heuristic triggers before invoking AI-powered diagnostics. Results are delivered through macOS native notifications.

The system prioritizes privacy by supporting local AI inference via Ollama while also offering cloud-based options. The architecture emphasizes fault tolerance, ensuring that failures in individual components do not compromise overall monitoring capabilities.

## Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     System Observer                          │
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │ Log Stream   │  │  Metrics     │  │   Config     │     │
│  │  Collector   │  │  Collector   │  │   Manager    │     │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘     │
│         │                  │                  │              │
│         └──────────┬───────┴──────────────────┘              │
│                    ▼                                         │
│         ┌─────────────────────┐                             │
│         │   Event Aggregator  │                             │
│         │   (Rolling Buffer)  │                             │
│         └──────────┬──────────┘                             │
│                    ▼                                         │
│         ┌─────────────────────┐                             │
│         │   Trigger Logic     │                             │
│         └──────────┬──────────┘                             │
│                    ▼                                         │
│         ┌─────────────────────┐                             │
│         │    AI Analyzer      │                             │
│         │  (Ollama/OpenAI)    │                             │
│         └──────────┬──────────┘                             │
│                    ▼                                         │
│         ┌─────────────────────┐                             │
│         │   Alert Manager     │                             │
│         └─────────────────────┘                             │
└─────────────────────────────────────────────────────────────┘
```

### Threading Model

- **Main Thread**: Coordinates component lifecycle and handles graceful shutdown
- **Log Collector Thread**: Spawns and monitors `log stream` subprocess, parses JSON output
- **Metrics Collector Thread**: Spawns and monitors `powermetrics` subprocess, parses plist/JSON output
- **Analysis Thread**: Consumes events from the aggregator, applies trigger logic, invokes AI
- **Notification Thread**: Delivers alerts asynchronously to avoid blocking analysis

Communication between threads uses Rust's `mpsc` channels for type-safe message passing.

## Components and Interfaces

### 1. Log Stream Collector

**Responsibility**: Interface with macOS Unified Log System and parse log entries.

**Interface**:
```rust
pub struct LogCollector {
    predicate: String,
    output_channel: Sender<LogEvent>,
}

pub struct LogEvent {
    timestamp: DateTime<Utc>,
    message_type: MessageType,
    subsystem: String,
    process: String,
    message: String,
}

pub enum MessageType {
    Error,
    Fault,
    Info,
    Debug,
}

impl LogCollector {
    pub fn new(predicate: String, channel: Sender<LogEvent>) -> Self;
    pub fn start(&mut self) -> Result<(), CollectorError>;
    pub fn stop(&mut self) -> Result<(), CollectorError>;
}
```

**Implementation Details**:
- Spawns `log stream --predicate <filter> --style json` as a subprocess
- Reads stdout line-by-line, parsing JSON arrays
- Handles partial reads and malformed JSON gracefully
- Automatically restarts subprocess on unexpected termination

### 2. Metrics Collector

**Responsibility**: Gather CPU, memory, GPU, and energy consumption metrics.

**Interface**:
```rust
pub struct MetricsCollector {
    sample_interval: Duration,
    output_channel: Sender<MetricsEvent>,
}

pub struct MetricsEvent {
    timestamp: DateTime<Utc>,
    cpu_usage: f64,
    memory_pressure: MemoryPressure,
    gpu_usage: Option<f64>,
    energy_impact: f64,
}

pub enum MemoryPressure {
    Normal,
    Warning,
    Critical,
}

impl MetricsCollector {
    pub fn new(interval: Duration, channel: Sender<MetricsEvent>) -> Self;
    pub fn start(&mut self) -> Result<(), CollectorError>;
    pub fn stop(&mut self) -> Result<(), CollectorError>;
}
```

**Implementation Details**:
- Spawns `sudo powermetrics --samplers cpu_power,gpu_power --format plist` as subprocess
- Parses plist output using `plist` crate
- Enters degraded mode if powermetrics unavailable
- Requires elevated privileges (handled via sudo or setuid)

### 3. Event Aggregator (Rolling Buffer)

**Responsibility**: Store recent events with time-based expiration and provide query interface.

**Interface**:
```rust
pub struct EventAggregator {
    log_buffer: VecDeque<LogEvent>,
    metrics_buffer: VecDeque<MetricsEvent>,
    max_age: Duration,
    max_size: usize,
}

impl EventAggregator {
    pub fn new(max_age: Duration, max_size: usize) -> Self;
    pub fn add_log(&mut self, event: LogEvent);
    pub fn add_metric(&mut self, event: MetricsEvent);
    pub fn get_recent_logs(&self, duration: Duration) -> Vec<&LogEvent>;
    pub fn get_recent_metrics(&self, duration: Duration) -> Vec<&MetricsEvent>;
    pub fn prune_old_entries(&mut self);
}
```

**Implementation Details**:
- Uses `VecDeque` for efficient insertion and removal
- Automatically prunes entries older than `max_age` on each insertion
- Enforces `max_size` limit by removing oldest entries when capacity reached

### 4. Trigger Logic

**Responsibility**: Apply heuristic rules to determine when AI analysis should be invoked.

**Interface**:
```rust
pub struct TriggerEngine {
    rules: Vec<Box<dyn TriggerRule>>,
}

pub trait TriggerRule: Send {
    fn evaluate(&self, aggregator: &EventAggregator) -> Option<TriggerContext>;
}

pub struct TriggerContext {
    pub reason: String,
    pub relevant_logs: Vec<LogEvent>,
    pub relevant_metrics: Vec<MetricsEvent>,
}

impl TriggerEngine {
    pub fn new() -> Self;
    pub fn add_rule(&mut self, rule: Box<dyn TriggerRule>);
    pub fn check_triggers(&self, aggregator: &EventAggregator) -> Option<TriggerContext>;
}
```

**Built-in Rules**:
- **ErrorFrequencyRule**: Triggers when error count exceeds threshold in time window
- **MemoryPressureRule**: Triggers when memory pressure reaches Warning or Critical
- **CrashDetectionRule**: Triggers on process crash indicators in logs
- **ResourceSpikeRule**: Triggers on sudden CPU/GPU usage increases

### 5. AI Analyzer

**Responsibility**: Format prompts and communicate with LLM backends for diagnostic analysis.

**Interface**:
```rust
pub struct AIAnalyzer {
    backend: Box<dyn LLMBackend>,
}

pub trait LLMBackend: Send {
    fn analyze(&self, context: &TriggerContext) -> Result<AIInsight, AnalysisError>;
}

pub struct AIInsight {
    pub summary: String,
    pub root_cause: Option<String>,
    pub recommendations: Vec<String>,
    pub severity: Severity,
}

pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl AIAnalyzer {
    pub fn new(backend: Box<dyn LLMBackend>) -> Self;
    pub fn analyze(&self, context: TriggerContext) -> Result<AIInsight, AnalysisError>;
}
```

**Backend Implementations**:
- **OllamaBackend**: Communicates with local Ollama API via HTTP
- **OpenAIBackend**: Communicates with OpenAI API with authentication
- **MockBackend**: Returns canned responses for testing

**Prompt Template**:
```
You are a macOS system diagnostics expert. Analyze the following system data and provide:
1. A concise summary of the issue
2. The likely root cause
3. Actionable recommendations

System Context:
- Time Window: {duration}
- Error Count: {count}
- Memory Pressure: {pressure}

Recent Errors:
{log_entries}

Recent Metrics:
{metrics}

Respond in JSON format with fields: summary, root_cause, recommendations (array), severity (info/warning/critical).
```

### 6. Alert Manager

**Responsibility**: Deliver notifications to the user via macOS notification system.

**Interface**:
```rust
pub struct AlertManager {
    rate_limiter: RateLimiter,
}

pub struct RateLimiter {
    max_per_minute: usize,
    recent_alerts: VecDeque<DateTime<Utc>>,
}

impl AlertManager {
    pub fn new(max_per_minute: usize) -> Self;
    pub fn send_alert(&mut self, insight: &AIInsight) -> Result<(), AlertError>;
}
```

**Implementation Details**:
- Uses `osascript` to trigger native notifications via AppleScript
- Rate limits to prevent notification spam (default: 3 per minute)
- Queues alerts when rate limit exceeded
- Formats notification with title (summary) and body (recommendations)

### 7. Configuration Manager

**Responsibility**: Load and validate application configuration.

**Interface**:
```rust
pub struct Config {
    pub log_predicate: String,
    pub metrics_interval: Duration,
    pub buffer_max_age: Duration,
    pub buffer_max_size: usize,
    pub error_threshold: usize,
    pub error_window: Duration,
    pub memory_threshold: MemoryPressure,
    pub ai_backend: AIBackendConfig,
    pub alert_rate_limit: usize,
}

pub enum AIBackendConfig {
    Ollama { endpoint: String, model: String },
    OpenAI { api_key: String, model: String },
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError>;
    pub fn default() -> Self;
}
```

**Configuration Format** (TOML):
```toml
[logging]
predicate = "messageType == error OR messageType == fault"

[metrics]
interval_seconds = 5

[buffer]
max_age_seconds = 60
max_size = 1000

[triggers]
error_threshold = 5
error_window_seconds = 10
memory_threshold = "Warning"

[ai]
backend = "ollama"
endpoint = "http://localhost:11434"
model = "llama3"

[alerts]
rate_limit_per_minute = 3
```

## Data Models

### Core Event Types

```rust
// Timestamp wrapper for consistent time handling
pub type Timestamp = DateTime<Utc>;

// Log event from Unified Log System
pub struct LogEvent {
    pub timestamp: Timestamp,
    pub message_type: MessageType,
    pub subsystem: String,
    pub category: String,
    pub process: String,
    pub process_id: u32,
    pub message: String,
}

// Metrics snapshot
pub struct MetricsEvent {
    pub timestamp: Timestamp,
    pub cpu_usage: f64,           // Percentage 0-100
    pub memory_pressure: MemoryPressure,
    pub memory_used_gb: f64,
    pub gpu_usage: Option<f64>,   // Percentage 0-100, None if unavailable
    pub energy_impact: f64,       // Arbitrary units from powermetrics
}

// Analysis trigger context
pub struct TriggerContext {
    pub trigger_time: Timestamp,
    pub reason: String,
    pub relevant_logs: Vec<LogEvent>,
    pub relevant_metrics: Vec<MetricsEvent>,
}

// AI analysis result
pub struct AIInsight {
    pub analysis_time: Timestamp,
    pub summary: String,
    pub root_cause: Option<String>,
    pub recommendations: Vec<String>,
    pub severity: Severity,
}
```

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system—essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*


### Property 1: Log parsing preserves structure
*For any* valid JSON log entry from the Unified Log System, parsing should successfully extract all required fields (timestamp, message_type, subsystem, process, message) without data loss.
**Validates: Requirements 1.2**

### Property 2: Malformed entries don't halt processing
*For any* malformed or invalid JSON input, the log parser should skip the entry and continue processing subsequent entries without crashing or blocking.
**Validates: Requirements 1.4**

### Property 3: Error and fault entries are captured
*For any* log entry with message_type of Error or Fault, the entry should be stored in the rolling buffer for analysis.
**Validates: Requirements 1.3, 3.5**

### Property 4: Metrics parsing extracts all fields
*For any* valid powermetrics output (plist or JSON format), parsing should successfully extract CPU usage, memory pressure, GPU usage, and energy impact values.
**Validates: Requirements 2.2**

### Property 5: Memory pressure threshold triggers analysis
*For any* metrics event where memory_pressure exceeds the configured threshold, the trigger logic should flag the condition for AI analysis.
**Validates: Requirements 2.5**

### Property 6: Rolling buffer maintains time-based expiration
*For any* sequence of log events added to the rolling buffer, querying for events within a time window should return only events with timestamps within that window.
**Validates: Requirements 3.1**

### Property 7: Rolling buffer enforces capacity limits
*For any* rolling buffer at maximum capacity, adding a new entry should result in the oldest entry being removed and the buffer size remaining at the maximum.
**Validates: Requirements 3.2**

### Property 8: Trigger activation on threshold breach
*For any* combination of error frequency and resource consumption, when either exceeds its configured threshold within the time window, the trigger logic should activate AI analysis.
**Validates: Requirements 3.3, 3.4**

### Property 9: Prompt formatting includes context
*For any* trigger context with log events and metrics, the formatted prompt should include the time window, event counts, and formatted representations of all relevant logs and metrics.
**Validates: Requirements 4.1**

### Property 10: AI backend receives analysis requests
*For any* trigger activation, the configured AI backend's analyze method should be invoked with the trigger context.
**Validates: Requirements 4.2**

### Property 11: LLM response extraction
*For any* valid LLM response containing summary, root_cause, recommendations, and severity fields, the AI analyzer should successfully extract all fields into an AIInsight structure.
**Validates: Requirements 4.5**

### Property 12: Critical issues trigger notifications
*For any* AIInsight with severity level of Critical, the alert manager should trigger a macOS notification.
**Validates: Requirements 5.1**

### Property 13: Notification content completeness
*For any* AIInsight, the generated notification should include the summary in the title and all recommendations in the body.
**Validates: Requirements 5.2, 5.3**

### Property 14: Notification failures don't halt operation
*For any* notification delivery failure, the alert manager should log the error and continue accepting new alerts without crashing.
**Validates: Requirements 5.4**

### Property 15: Rate limiting prevents spam
*For any* sequence of alerts exceeding the rate limit, only the maximum allowed number of notifications should be delivered within the time window, with excess alerts queued or dropped.
**Validates: Requirements 5.5**

### Property 16: Configuration values are applied
*For any* valid configuration specifying thresholds, time windows, or backend type, the corresponding components should use the configured values instead of defaults.
**Validates: Requirements 6.2, 6.3, 6.4**

### Property 17: Invalid configuration uses safe defaults
*For any* invalid configuration file (malformed, missing required fields, or out-of-range values), the system should report errors and initialize with safe default values.
**Validates: Requirements 6.5**

### Property 18: Log stream restart on failure
*For any* unexpected termination of the log stream subprocess, the log collector should detect the failure and attempt to restart the connection.
**Validates: Requirements 7.1**

### Property 19: AI backend failures are queued for retry
*For any* AI analysis request when the backend is unreachable, the analyzer should log the failure and queue the request for retry without blocking new triggers.
**Validates: Requirements 7.3**

## Error Handling

### Error Categories

1. **Recoverable Errors**: Automatically retried with exponential backoff
   - Subprocess crashes (log stream, powermetrics)
   - AI backend timeouts or connection failures
   - Temporary filesystem issues

2. **Degraded Mode Errors**: System continues with reduced functionality
   - powermetrics unavailable (continue with log monitoring only)
   - AI backend permanently unavailable (log triggers but skip analysis)
   - Notification delivery failures (log but continue monitoring)

3. **Fatal Errors**: Graceful shutdown with error reporting
   - Configuration file critically malformed
   - Unable to allocate rolling buffer
   - Insufficient permissions for log access

### Error Handling Strategies

**Subprocess Management**:
- Monitor subprocess health via periodic heartbeat checks
- Capture stderr for diagnostic logging
- Implement restart with exponential backoff (1s, 2s, 4s, 8s, max 60s)
- After 5 consecutive failures, enter degraded mode

**AI Backend Resilience**:
- Set request timeout (default: 30 seconds)
- Implement retry queue with maximum size (default: 100 requests)
- On persistent failures, log warning and continue monitoring
- Provide fallback to rule-based alerting when AI unavailable

**Resource Exhaustion**:
- Monitor own memory usage via `rusage`
- If memory usage exceeds threshold, reduce buffer sizes
- If CPU usage exceeds threshold, increase sampling intervals
- Log resource pressure warnings

**Notification Failures**:
- Catch and log osascript execution errors
- Implement notification queue with maximum size
- Drop oldest queued notifications if queue full
- Continue monitoring regardless of notification status

## Testing Strategy

### Unit Testing

The system will use Rust's built-in testing framework (`cargo test`) for unit tests. Unit tests will focus on:

- **Parser correctness**: Verify JSON and plist parsing with known valid inputs
- **Configuration loading**: Test valid and invalid configuration files
- **Buffer operations**: Test insertion, expiration, and capacity enforcement with specific sequences
- **Trigger rule evaluation**: Test each trigger rule with specific event patterns
- **Prompt formatting**: Verify prompt template rendering with sample data
- **Rate limiter logic**: Test rate limiting with specific timing sequences

Unit tests will use mock implementations of external dependencies (subprocess execution, AI backends, notification delivery) to enable fast, deterministic testing.

### Property-Based Testing

The system will use **quickcheck** (https://github.com/BurntSushi/quickcheck) for property-based testing in Rust. Property-based tests will be configured to run a minimum of 100 iterations per property.

Each property-based test will be tagged with a comment explicitly referencing the correctness property from this design document using the format: `// Feature: macos-system-observer, Property N: <property text>`

Property-based tests will verify:

- **Property 1**: Generate random valid JSON log structures and verify parsing
- **Property 2**: Generate random malformed JSON and verify graceful handling
- **Property 3**: Generate random log entries with various message types and verify filtering
- **Property 4**: Generate random powermetrics output and verify parsing
- **Property 5**: Generate random metrics with various memory pressure levels and verify triggering
- **Property 6**: Generate random event sequences and verify time-based queries
- **Property 7**: Generate random event sequences exceeding capacity and verify FIFO behavior
- **Property 8**: Generate random event patterns and verify trigger activation
- **Property 9**: Generate random trigger contexts and verify prompt completeness
- **Property 10**: Generate random trigger contexts and verify backend invocation
- **Property 11**: Generate random LLM responses and verify extraction
- **Property 12**: Generate random insights with various severities and verify notification triggering
- **Property 13**: Generate random insights and verify notification content
- **Property 14**: Simulate random notification failures and verify continued operation
- **Property 15**: Generate random alert sequences and verify rate limiting
- **Property 16**: Generate random valid configurations and verify application
- **Property 17**: Generate random invalid configurations and verify default fallback
- **Property 18**: Simulate random subprocess failures and verify restart behavior
- **Property 19**: Simulate random backend failures and verify retry queuing

### Integration Testing

Integration tests will verify end-to-end behavior:

- Spawn actual `log stream` subprocess with test predicates
- Verify log events flow through the pipeline to trigger evaluation
- Test configuration loading from actual TOML files
- Verify notification delivery via osascript (in test mode)

### Test Utilities

The test suite will include:

- **Mock LLM Backend**: Returns canned responses for deterministic testing
- **Event Generators**: Create realistic log and metrics events for testing
- **Time Manipulation**: Control time progression for testing time-based behavior
- **Subprocess Mocks**: Simulate subprocess output without spawning actual processes

## Dependencies

### Core Dependencies

- **tokio** (1.x): Async runtime for concurrent operations
- **serde** (1.x): Serialization/deserialization for JSON and configuration
- **serde_json** (1.x): JSON parsing for log entries
- **plist** (1.x): Plist parsing for powermetrics output
- **chrono** (0.4): Date and time handling
- **toml** (0.8): Configuration file parsing
- **reqwest** (0.11): HTTP client for AI backend communication
- **anyhow** (1.x): Error handling
- **thiserror** (1.x): Custom error types
- **log** (0.4): Logging facade
- **env_logger** (0.11): Logging implementation

### Testing Dependencies

- **quickcheck** (1.x): Property-based testing framework
- **mockall** (0.12): Mocking framework for unit tests
- **tempfile** (3.x): Temporary file creation for config tests

### Platform Dependencies

- **macOS 10.15+**: Required for Unified Log System access
- **sudo privileges**: Required for powermetrics execution (optional, degrades gracefully)

## Deployment Considerations

### Installation

The application will be distributed as:
1. **Standalone binary**: Single executable with embedded defaults
2. **Homebrew formula**: For easy installation and updates
3. **Source build**: Via `cargo install`

### Permissions

- **Log access**: Requires Full Disk Access permission in macOS System Preferences
- **Notifications**: Requires notification permission (requested on first alert)
- **Elevated privileges**: Optional for powermetrics; uses sudo or setuid wrapper

### Configuration

Default configuration location: `~/.config/macos-system-observer/config.toml`

Users can override with `--config` flag or `SYSTEM_OBSERVER_CONFIG` environment variable.

### Running as Service

The application can run as:
1. **Foreground process**: For testing and development
2. **launchd daemon**: For persistent background monitoring
3. **Login item**: For user-session monitoring

Example launchd plist will be provided for automatic startup.

## Future Enhancements

- **Web dashboard**: Real-time visualization of system health
- **Historical analysis**: Store events in SQLite for trend analysis
- **Custom trigger rules**: User-defined trigger logic via configuration
- **Multi-machine monitoring**: Aggregate data from multiple Macs
- **Disk usage monitoring**: Integrate filesystem event monitoring
- **Network activity tracking**: Monitor bandwidth and connection patterns
