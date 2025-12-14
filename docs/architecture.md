# Architecture

Eyes uses a hybrid multi-threaded and async architecture with clear separation between data collection, analysis, and notification delivery.

## Threading and Async Model

- **Main Thread**: Hosts the `SystemObserver` orchestrator, coordinates component lifecycle and handles graceful shutdown
- **Log Collector Thread**: Spawns and monitors `log stream` subprocess, parses JSON output with intelligent restart on failure
- **Metrics Collector Thread**: Spawns and monitors `powermetrics` with fallback to `top`/`vm_stat` when powermetrics is unavailable
- **Disk Collector Thread**: Spawns and monitors `iostat` plus best-effort `fs_usage` when sudo is available
- **Analysis Thread**: Consumes events from the aggregator, applies trigger logic, invokes AI backends via a shared Tokio runtime
- **Notification Thread**: Polls the alert manager's queue (`tick()`) to deliver queued alerts respecting rate limits

## Application Orchestration

The `SystemObserver` serves as the central coordinator that manages all system components:

```rust
pub struct SystemObserver {
    config: Config,
    log_collector: LogCollector,
    metrics_collector: MetricsCollector,
    event_aggregator: Arc<Mutex<EventAggregator>>,
    trigger_engine: TriggerEngine,
    ai_analyzer: AIAnalyzer,
    alert_manager: Arc<Mutex<AlertManager>>,
    self_monitoring: Arc<SelfMonitoringCollector>,
    // Communication channels and thread handles
}
```

**Key Responsibilities:**
- **Component Initialization**: Creates and configures all system components based on loaded configuration
- **Communication Setup**: Establishes MPSC channels for inter-component communication
- **Lifecycle Management**: Coordinates startup, shutdown, and error recovery across all components
- **Configuration Integration**: Maps configuration sections to appropriate component settings
- **Self-Monitoring**: Tracks application performance metrics and health indicators
- **Thread Safety**: Manages shared state using Arc/Mutex patterns where needed

See [Application Orchestration](application-orchestration.md) for detailed implementation.

## Data Flow

```
┌─────────────────┐
│  log stream     │ (macOS Unified Logs)
└────────┬────────┘
         │ JSON events
         ▼
┌─────────────────┐
│ Log Collector   │
└────────┬────────┘
         │
         ├──────────────┐
         │              │
         ▼              ▼
┌─────────────────┐  ┌─────────────────┐
│ powermetrics    │  │ Event           │
│ or top/vm_stat  │  │ Aggregator      │
└────────┬────────┘  │ (Rolling Buffer)│
         │ plist/text └────────┬────────┘
         ▼                    │
┌─────────────────┐           │
│ Metrics         │           │
│ Collector       │           │
└────────┬────────┘           │
         │                    │
         ├──────────────┐     │
         │              │     │
         ▼              │     │
┌─────────────────┐     │     │
│ iostat /        │     │     │
│ fs_usage (opt)  │─────┘     │
└────────┬────────┘           │
         │                    │
┌─────────────────┐           │
│ Disk Collector  │───────────┘
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Trigger Engine  │
└────────┬────────┘
         │ when threshold exceeded
         ▼
┌─────────────────┐
│  AI Analyzer    │ ←─── async HTTP requests via shared Tokio runtime
│ (Ollama/OpenAI) │
└────────┬────────┘
         │ insights
         ▼
┌─────────────────┐
│ Alert Manager   │ ←─── polled by notification thread
│ (with queueing) │
└────────┬────────┘
         │
         ▼
macOS Notifications
(rate-limited)
```

**Self-Monitoring Integration**: The `SelfMonitoringCollector` runs as a cross-cutting concern, collecting performance metrics from all components:
- **Memory Usage**: Application-wide memory consumption tracking
- **Event Processing**: Counts of log/metrics events processed per minute
- **AI Analysis Latency**: Timing of AI backend operations
- **Notification Success Rates**: Alert delivery effectiveness tracking

## Communication

The system uses multiple communication patterns:

### Thread Communication
Threads communicate via Rust's `mpsc` channels for type-safe message passing:

- `Sender<LogEvent>`: Log collector → log forwarding thread → Analysis thread
- `Sender<MetricsEvent>`: Metrics collector → metrics forwarding thread → Analysis thread
- `Sender<DiskEvent>`: Disk collector → disk forwarding thread → Analysis thread
- `Sender<AnalysisMessage>`: Forwarding threads → Analysis thread, where events are applied to the aggregator and triggers are evaluated

### Async Communication
Async work is limited to AI backend HTTP calls inside the analysis thread's shared Tokio runtime. Other components use blocking I/O on dedicated threads with `Arc<Mutex<T>>` for shared state where needed. See [Async Processing](async-processing.md) for concurrency patterns.

## Event Aggregator

The Event Aggregator maintains rolling buffers of recent events with dual constraints:

**Time-Based Expiration**: Events older than `buffer.max_age_seconds` (default: 60s) are automatically pruned.

**Capacity Limits**: Each buffer (logs and metrics) is capped at `buffer.max_size` (default: 1000 events).

This design ensures:
- Bounded memory usage even during event bursts
- Temporal context for AI analysis (recent history)
- Fast time-windowed queries for trigger evaluation
- Automatic cleanup without manual intervention

The aggregator is single-threaded and accessed only by the analysis thread, avoiding lock contention. Collectors send events via channels rather than direct access.

See [Event Aggregation](event-aggregation.md) for implementation details.

## Async Processing Architecture

### AI Backend Integration

AI analysis is fully asynchronous to prevent blocking system monitoring:

```rust
// Non-blocking AI analysis
let insight = ai_analyzer.analyze(&trigger_context).await?;
```

Key benefits:
- **Non-blocking**: System monitoring continues during AI analysis
- **Concurrent Analysis**: Multiple AI requests can be processed simultaneously
- **Timeout Handling**: Requests have configurable timeouts to prevent hanging
- **Error Recovery**: Failed requests don't block subsequent analysis

### Alert Queue Processing

The alert manager processes queued alerts synchronously and is polled by a dedicated notification thread:

```rust
// Immediate processing happens inside send_alert (queues if rate limited)
alert_manager.send_alert(&insight)?;

// Background processing driven by the notification thread
loop {
    alert_manager.tick()?; // Processes queued alerts if rate limits allow
    std::thread::sleep(Duration::from_millis(500));
}
```

### Shared State Management

Components requiring shared access use Arc/Mutex patterns:

```rust
let alert_manager = Arc::new(Mutex::new(AlertManager::new(3)));
let manager_clone = Arc::clone(&alert_manager);

// Safe concurrent access across async tasks
tokio::spawn(async move {
    let mut manager = manager_clone.lock().unwrap();
    manager.send_alert(&insight).unwrap();
});
```

See [Async Processing](async-processing.md) for detailed patterns and best practices.

## Error Handling Strategy

### Recoverable Errors
Automatically retried with exponential backoff:
- Subprocess crashes (log stream, powermetrics)
- AI backend timeouts or connection failures
- Temporary filesystem issues

### Degraded Mode
System continues with reduced functionality:
- powermetrics unavailable → switch to `top`/`vm_stat` fallback metrics (no GPU metrics)
- AI backend unavailable → log triggers but skip analysis
- Notification failures → queue alerts for retry, continue monitoring
- Async task failures → restart background tasks, maintain core functionality

### Fatal Errors
Graceful shutdown with error reporting:
- Configuration critically malformed
- Unable to allocate rolling buffer
- Insufficient permissions for log access
