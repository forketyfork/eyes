# Self-Monitoring System

Eyes includes a comprehensive self-monitoring system that tracks the application's own performance and health metrics. This enables proactive detection of issues within the monitoring system itself.

## Overview

The self-monitoring system collects and analyzes metrics about Eyes' internal operations:

- **Memory Usage**: Current application memory consumption
- **Event Processing Rates**: Log and metrics events processed per minute
- **AI Analysis Latency**: Average time for AI backend analysis operations
- **Notification Delivery**: Success/failure rates for alert delivery
- **Performance Warnings**: Automatic detection of performance degradation

## Core Components

### SelfMonitoringCollector

The central component that tracks application metrics:

```rust
use eyes::monitoring::SelfMonitoringCollector;

let collector = SelfMonitoringCollector::new();

// Record AI analysis timing
collector.record_ai_analysis_latency(Duration::from_millis(150));

// Record notification delivery results
collector.record_notification_result(true);  // Success
collector.record_notification_result(false); // Failure

// Record event processing counts
collector.record_log_events_processed(25);
collector.record_metrics_events_processed(12);

// Collect current metrics
let metrics = collector.collect_metrics();
```

### SelfMonitoringMetrics

Structured metrics data collected by the system:

```rust
pub struct SelfMonitoringMetrics {
    pub memory_usage_bytes: u64,
    pub log_events_per_minute: u64,
    pub metrics_events_per_minute: u64,
    pub avg_ai_analysis_latency_ms: f64,
    pub successful_notifications_per_minute: u64,
    pub failed_notifications_per_minute: u64,
    pub notification_success_rate: f64,
    pub timestamp: DateTime<Utc>,
}
```

### AnalysisTimer

Helper for automatically timing AI analysis operations:

```rust
use eyes::monitoring::AnalysisTimer;

// Start timing
let timer = AnalysisTimer::start(collector);

// Perform AI analysis
let result = ai_analyzer.analyze(&context).await;

// Automatically record latency when timer is dropped
timer.finish();
```

## Integration Points

### Alert Manager Integration

The AlertManager automatically tracks notification delivery success rates:

- Records successful notification deliveries
- Records failed notification attempts
- Calculates success rates over time windows
- Provides metrics for notification system health

### AI Analyzer Integration

The AIAnalyzer automatically tracks analysis performance across all execution contexts:

- Records analysis latency for each AI backend operation
- Integrates with application-wide self-monitoring system in both main and analysis threads
- Provides metrics for AI system health monitoring
- Automatic performance warnings when latency exceeds thresholds
- Thread-safe monitoring ensures consistent tracking regardless of execution context

The AI analyzer is configured with self-monitoring in multiple contexts:
- **Main Thread**: During initial SystemObserver setup
- **Analysis Thread**: During background analysis thread initialization for comprehensive coverage

This ensures that AI analysis performance is tracked consistently, whether analysis occurs in the main application thread or in dedicated background analysis threads.

### Main Application Integration

The SystemObserver exposes self-monitoring metrics:

```rust
// Get current self-monitoring metrics
let metrics = system_observer.get_self_monitoring_metrics();

println!("Memory usage: {}MB", metrics.memory_usage_bytes / 1024 / 1024);
println!("AI latency: {:.1}ms", metrics.avg_ai_analysis_latency_ms);
println!("Notification success: {:.1}%", metrics.notification_success_rate);
```

### Thread-Safe Sharing

The collector can be safely shared across threads:

```rust
let collector = Arc::new(SelfMonitoringCollector::new());

// Clone for use in different threads
let collector_clone = collector.clone_collector();

// Use in async contexts
tokio::spawn(async move {
    collector_clone.record_notification_result(true);
});
```

## Metrics Collection

### Memory Usage

Tracks application memory consumption using platform-specific methods:

- **macOS**: Uses `ps -o rss` command to get current resident set size
- **Linux**: Reads from `/proc/self/status` (VmRSS field)
- **Fallback**: Uses `getrusage()` system call (reports peak usage, not current)
- **Ultimate fallback**: Returns 0 if memory usage cannot be determined

### Event Processing Rates

Monitors data flow through the system:

- **Log Events**: Count of log entries processed per minute
- **Metrics Events**: Count of system metrics processed per minute
- **Time Windows**: Uses 1-minute sliding windows for rate calculations
- **Automatic Cleanup**: Removes old event count buckets automatically

### AI Analysis Latency

Tracks performance of AI backend operations:

- **Sample Collection**: Maintains last 100 analysis operations
- **Average Calculation**: Computes mean latency across recent samples
- **Automatic Rotation**: Removes oldest samples when limit exceeded

### Notification Success Rates

Monitors alert delivery effectiveness:

- **Success Tracking**: Records successful notification deliveries
- **Failure Tracking**: Records failed notification attempts
- **Rate Calculation**: Computes success percentage over 1-minute windows
- **Sample Limits**: Maintains last 1000 notification results

## Performance Warnings

The system automatically detects and warns about performance issues:

### High Memory Usage
```
WARN High memory usage detected: 512MB
```
Triggered when memory usage exceeds 500MB.

### High AI Analysis Latency
```
WARN High AI analysis latency detected: 35000.0ms
```
Triggered when average latency exceeds 30 seconds.

### Low Notification Success Rate
```
WARN Low notification success rate: 75.5%
```
Triggered when success rate drops below 90% (with active notifications).

## Configuration

Self-monitoring is enabled by default with these limits:

- **Max Latency Samples**: 100 operations
- **Max Notification Samples**: 1000 results
- **Max Count Age**: 5 minutes for event count buckets

These limits ensure bounded memory usage while providing sufficient data for meaningful metrics.

## Logging Integration

Self-monitoring integrates with the application's logging system:

```bash
# Enable debug logging to see detailed self-monitoring operations
RUST_LOG=debug cargo run

# Or via CLI flag
cargo run -- --verbose
```

Debug logs show:
- Metric recording operations
- Sample collection and rotation
- Performance warning triggers
- Memory usage calculations

## Use Cases

### Performance Monitoring

Track application performance over time:
- Monitor memory usage trends
- Identify AI analysis performance degradation
- Detect notification delivery issues

### Capacity Planning

Understand system resource requirements:
- Peak memory usage patterns
- Event processing throughput
- AI backend response times

### Troubleshooting

Diagnose application issues:
- High latency in AI analysis
- Memory leaks or excessive usage
- Notification delivery failures

### Health Checks

Implement health monitoring:
- Verify all components are processing events
- Ensure notification system is functional
- Monitor overall system responsiveness

## Testing

The self-monitoring system includes comprehensive tests:

```bash
# Run self-monitoring tests
cargo test monitoring

# Test with property-based testing
cargo test --release monitoring
```

Test coverage includes:
- Metric collection accuracy
- Sample rotation and limits
- Thread safety and sharing
- Timer functionality
- Memory usage calculation