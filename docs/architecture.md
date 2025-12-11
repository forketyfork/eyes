# Architecture

Eyes uses a multi-threaded producer-consumer architecture with clear separation between data collection, analysis, and notification delivery.

## Threading Model

- **Main Thread**: Coordinates component lifecycle and handles graceful shutdown
- **Log Collector Thread**: Spawns and monitors `log stream` subprocess, parses JSON output with intelligent restart on failure
- **Metrics Collector Thread**: Spawns and monitors `powermetrics` subprocess, parses plist/JSON output
- **Analysis Thread**: Consumes events from the aggregator, applies trigger logic, invokes AI
- **Notification Thread**: Delivers alerts asynchronously to avoid blocking analysis

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
         │  AI Analyzer    │
         │ (Ollama/OpenAI) │
         └────────┬────────┘
                  │ insights
                  ▼
         ┌─────────────────┐
         │ Alert Manager   │
         └────────┬────────┘
                  │
                  ▼
         macOS Notifications
```

## Communication

Threads communicate via Rust's `mpsc` channels for type-safe message passing:

- `Sender<LogEvent>`: Log collector → Event aggregator
- `Sender<MetricsEvent>`: Metrics collector → Event aggregator
- `Sender<TriggerContext>`: Trigger engine → AI analyzer
- `Sender<AIInsight>`: AI analyzer → Alert manager

See [Data Models](data-models.md) for detailed type definitions.

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
- Notification failures → log but continue monitoring

### Fatal Errors
Graceful shutdown with error reporting:
- Configuration critically malformed
- Unable to allocate rolling buffer
- Insufficient permissions for log access
