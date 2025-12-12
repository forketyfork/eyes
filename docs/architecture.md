# Architecture

Eyes uses a hybrid multi-threaded and async architecture with clear separation between data collection, analysis, and notification delivery.

## Threading and Async Model

- **Main Thread**: Coordinates component lifecycle and handles graceful shutdown
- **Log Collector Thread**: Spawns and monitors `log stream` subprocess, parses JSON output with intelligent restart on failure
- **Metrics Collector Thread**: Spawns and monitors `powermetrics` subprocess, parses plist/JSON output
- **Analysis Thread**: Consumes events from the aggregator, applies trigger logic, invokes AI backends asynchronously
- **Async Tasks**: AI backend communication, alert queue processing, and HTTP requests use tokio async runtime
- **Notification Processing**: Alert manager supports both synchronous and async processing with intelligent queueing

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
└────────┬────────┘  │ Aggregator      │
         │           │ (Rolling Buffer)│
         │ plist     └────────┬────────┘
         ▼                    │
┌─────────────────┐           │
│ Metrics         │           │
│ Collector       │           │
└────────┬────────┘           │
         │                    │
         └────────────────────┘
                    │
                    ▼
         ┌─────────────────┐
         │ Trigger Engine  │
         └────────┬────────┘
                  │ when threshold exceeded
                  ▼
         ┌─────────────────┐
         │  AI Analyzer    │ ←─── async HTTP requests
         │ (Ollama/OpenAI) │
         └────────┬────────┘
                  │ insights
                  ▼
         ┌─────────────────┐
         │ Alert Manager   │ ←─── background queue processing
         │ (with queueing) │      (tokio::spawn)
         └────────┬────────┘
                  │
                  ▼
         macOS Notifications
         (rate-limited)
```

## Communication

The system uses multiple communication patterns:

### Thread Communication
Threads communicate via Rust's `mpsc` channels for type-safe message passing:

- `Sender<LogEvent>`: Log collector → Event aggregator
- `Sender<MetricsEvent>`: Metrics collector → Event aggregator
- `Sender<TriggerContext>`: Trigger engine → AI analyzer
- `Sender<AIInsight>`: AI analyzer → Alert manager

### Async Communication
Async components use tokio primitives:

- **HTTP Requests**: AI backends communicate with LLM services via async HTTP
- **Shared State**: Alert manager uses `Arc<Mutex<T>>` for thread-safe shared access
- **Background Tasks**: Queue processing and periodic tasks use `tokio::spawn`
- **Timers**: Rate limiting and intervals use `tokio::time::interval`

See [Data Models](data-models.md) for detailed type definitions and [Async Processing](async-processing.md) for concurrency patterns.

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

The alert manager supports both immediate and background processing:

```rust
// Immediate processing
alert_manager.send_alert(&insight)?;

// Background queue processing
tokio::spawn(async move {
    let mut interval = interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        manager.process_queue()?;
    }
});
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
- powermetrics unavailable → continue with log monitoring only
- AI backend unavailable → log triggers but skip analysis
- Notification failures → queue alerts for retry, continue monitoring
- Async task failures → restart background tasks, maintain core functionality

### Fatal Errors
Graceful shutdown with error reporting:
- Configuration critically malformed
- Unable to allocate rolling buffer
- Insufficient permissions for log access
