# Alert System

Eyes persists every admitted trigger candidate, whether AI analysis succeeds or not, together with any completed assessment and notification lifecycle. Native macOS notifications are an explicit opt-in through `--enable-notifications` and remain disabled otherwise.

## Overview

The alert system coordinates between AI-generated insights and macOS notification delivery:

1. **Insight Evaluation**: Determine if an insight warrants a notification
2. **Rate Limiting**: Check if notification frequency limits allow delivery
3. **Formatting**: Create user-friendly notification content
4. **Delivery**: Send native macOS notifications via osascript
5. **Tracking**: Persist the trigger, analysis, and notification lifecycles

## AlertManager

The same persisted history is available in a local web dashboard at `http://127.0.0.1:8787` by default. The dashboard provides sortable, paginated alert groups and expandable details. Its paginated API returns summary fields and group counts; expanding a row loads assessment details, raw trigger evidence, agent history, and grouped alerts. Similar alerts attached by an agent are folded beneath their root alert in one collapsible section, while the counters continue to represent every signal. Agent reviews, resolution entries, and open/resolved state appear with the alert details. Candidates skipped because automatic analysis is disabled show as `not_done`; candidates awaiting AI show as `pending`; queue drops, exhausted retries, interrupted work, and persistence failures show as `failed`; completed assessments show as `analyzed`. Not-done and failed rows provide an **Analyze now** action that resubmits their persisted trigger context to the existing AI worker. Configure or disable the listener through the `[web]` section.

The central coordinator for notification delivery with built-in rate limiting, intelligent alert queueing, async processing capabilities, and self-monitoring integration.

### Key Features

- **Explicit Opt-in**: Desktop delivery requires `--enable-notifications`
- **Severity Filtering**: When enabled, the configured minimum severity controls notification delivery
- **Rate Limiting**: Prevents notification spam with configurable limits
- **Alert Queueing**: Queues rate-limited alerts for later delivery when capacity becomes available
- **Queue Management**: Configurable queue size with overflow handling (drops oldest alerts)
- **Background Processing**: Notification thread polls the queue with `tick()` to deliver alerts when rate limits allow
- **Thread Safety**: Arc/Mutex-based shared state management for concurrent access
- **Native Integration**: Uses osascript for authentic macOS notifications
- **Graceful Degradation**: Continues operation even if notifications fail
- **Self-Monitoring**: Automatic tracking of notification delivery success rates and performance metrics
- **Persistent History**: Structured SQLite storage for trigger candidates, analysis state, assessments, and delivery outcomes

### Usage

```rust
use eyes::alerts::AlertManager;
use eyes::ai::AIInsight;
use eyes::events::Severity;
use std::path::Path;
use std::sync::{Arc, Mutex};

let mut alert_manager = AlertManager::with_database(
    3,
    100,
    Severity::Warning,
    Path::new("eyes.db"),
)?;
alert_manager.set_desktop_notifications_enabled(true);

// Send an alert for a critical insight
match alert_manager.send_alert(&insight) {
    Ok(()) => println!("Notification sent or queued successfully"),
    Err(AlertError::NotificationFailed(msg)) => println!("Notification failed: {}", msg),
}

// Check queue status
println!("Queued alerts: {}", alert_manager.queued_alert_count());

// For concurrent usage with shared state
let alert_manager = Arc::new(Mutex::new(alert_manager));
let manager_clone = Arc::clone(&alert_manager);

// Can be used across threads
std::thread::spawn(move || {
    let mut manager = manager_clone.lock().unwrap();
    manager.send_alert(&insight).unwrap();
});
```

The main application always uses `with_database`. The constructors without a database remain available for tests and embedders that intentionally do not want persistence.

### Configuration

```toml
[alerts]
rate_limit_per_minute = 3  # Maximum notifications per minute
minimum_severity = "warning"  # Notify for warning and critical insights

[storage]
database_path = "eyes.db"
```

## SQLite Alert History

Eyes writes an `alert_candidates` row and the rule-selected trigger events before dispatching AI work. The evidence is available immediately and does not depend on AI: log context retains its timestamp, level, process/PID, subsystem, category, and exact message; metric and disk events retain their structured measurements. Successful analysis writes the assessment and notification alert in one transaction and links both records to the candidate. Runtime history writes are best-effort: a SQLite write failure is logged without suppressing notification delivery or queueing. When desktop notifications are not enabled, completed assessments are retained with a `suppressed` notification status. The same status is used for alerts below `minimum_severity`. Rate-limited alerts transition through `queued`; delivered and failed attempts become `delivered` or `delivery_failed`; notification queue overflow produces `dropped`.

Candidate analysis states are independent from notification states:

- `pending`: admitted by the trigger cooldown and waiting for the initial analysis or a retry
- `not_done`: automatic analysis was intentionally skipped and remains available on demand
- `analyzed`: linked to a completed AI assessment, whether or not it produced a notification
- `failed`: analysis never completed because the worker was busy or disconnected, retries were exhausted, Eyes stopped or restarted, or the completed assessment could not be persisted

Manual analysis is accepted for `not_done` and `failed` candidates. `POST /api/alerts/{candidate_id}/analyze` reconstructs the original `TriggerContext` from persisted evidence, conditionally changes the candidate to `pending`, and submits it to a bounded manual-analysis channel. Accepted retries remain pending in FIFO order while the AI worker is busy. Concurrent requests, pending work, and already analyzed candidates return a conflict instead of creating duplicate assessments.

The schema keeps trigger candidates separate from optional AI and notification records:

- `alert_candidates`: trigger time, rule, source, reason, expected severity, event counts, analysis state, resolution state, optional group parent, and optional assessment/alert links
- `alert_candidate_context_events`: ordered JSON payloads for the exact log, metric, and disk events selected by the trigger rule
- `alert_agent_reviews`: append-only agent reviews and resolution records
- `alerts`: notification title/body, lifecycle timestamps, status, and failure details
- `assessments`: timestamp, summary, root cause, severity, and confidence values
- `assessment_recommendations`: ordered recommended actions
- `assessment_evidence`: ordered supporting observations
- `assessment_limitations`: ordered caveats and alternative explanations

`alerts.assessment_id` is a unique foreign key, so each notification alert has exactly one attached assessment. An alert candidate may have neither link while pending, not done, or failed. Existing history is backfilled as analyzed legacy candidates, but raw trigger evidence cannot be reconstructed retroactively. The database enables foreign keys, uses WAL journaling, and tracks its migration with SQLite's `user_version`.

Grouping is deliberately one level deep. A root candidate has no `group_parent_id`; attached candidates reference the root. Attaching an existing root group to another root moves its children as well, so the dashboard and MCP responses never need recursive group rendering. Resolution is independent from analysis and notification delivery state. Resolving an alert changes it from `open` to `resolved` and appends the agent's resolution in the same transaction.

Example history query:

```sql
SELECT c.triggered_at, c.analysis_status,
       COALESCE(s.severity, c.expected_severity) AS severity,
       COALESCE(s.summary, c.trigger_reason) AS summary
FROM alert_candidates AS c
LEFT JOIN assessments AS s ON s.id = c.assessment_id
ORDER BY c.triggered_at DESC;
```

## MCP Server

`eyes-mcp` is a standalone stdio server backed by the same SQLite database as Eyes. Build it with `cargo build --release`, then configure an MCP client to run:

```text
/absolute/path/to/target/release/eyes-mcp --database /absolute/path/to/eyes.db
```

It exposes six tools:

- `list_alerts`: list alert summaries with optional severity and resolution filters
- `search_alerts`: text search over summaries, root causes, trigger metadata, and agent reviews
- `get_alert`: return the complete alert, including raw trigger events, AI assessment, delivery state, agent history, and grouped children
- `resolve_alert`: mark an open alert resolved and atomically append the agent's resolution
- `attach_similar_alerts`: fold one or more alerts under a root; existing child groups are flattened into the new root
- `append_agent_review`: append a review without changing the alert's resolution state

All alert IDs are `alert_candidates.id`, matching the signal IDs shown in the dashboard. List and search responses are bounded to 100 records per call and support offsets. Tool execution errors are returned as structured MCP tool errors so agents can correct their request.

## Alert Queueing System

The AlertManager includes intelligent alert queueing with async processing capabilities to handle rate-limited scenarios gracefully.

### Queue Behavior

- **Immediate Delivery**: Alerts are sent immediately if rate limits allow
- **Queue on Rate Limit**: When rate limited, critical alerts are queued for later processing
- **Automatic Processing**: Queued alerts are processed when rate limit capacity becomes available
- **Autonomous Processing**: The `tick()` method enables autonomous queue processing; the notification thread calls this periodically
- **Overflow Protection**: Queue has configurable maximum size (default: 100 alerts)
- **FIFO Processing**: Oldest queued alerts are processed first
- **Overflow Handling**: When queue is full, oldest alerts are dropped to make room for new ones
- **Independent Processing**: Queue can be processed independently via `process_queue()` or `tick()` methods

### Queue Management

```rust
// Check queue status
let queued_count = alert_manager.queued_alert_count();
let can_send_now = alert_manager.can_send_notification();

// The queue is automatically processed on each send_alert() call
alert_manager.send_alert(&new_insight)?; // Processes queue + handles new alert

// Autonomous processing - call periodically to process queued alerts
// even when no new alerts arrive
let processed_count = alert_manager.tick()?; // Returns number of alerts processed

// Check if there are processable alerts waiting
if alert_manager.has_processable_alerts() {
    println!("Queued alerts can be processed now");
}

// Manual queue processing (also available)
let processed = alert_manager.process_queue()?;
```

### Integration with Main Application Loop

The `tick()` method should be called periodically by the main application to ensure autonomous queue processing:

```rust
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use eyes::alerts::AlertManager;

let manager = Arc::new(Mutex::new(AlertManager::new(3)));
let mgr = Arc::clone(&manager);

thread::spawn(move || {
    loop {
        if let Ok(mut mgr) = mgr.lock() {
            let _ = mgr.tick();
        }
        thread::sleep(Duration::from_millis(500));
    }
});
```

## RateLimiter

Implements sliding window rate limiting to prevent notification spam.

### Algorithm

Uses a sliding window approach with automatic cleanup:

1. **Tracking**: Maintains timestamps of recent notifications in a VecDeque
2. **Cleanup**: Automatically removes notifications older than 1 minute
3. **Evaluation**: Checks if current count is below the configured limit
4. **Recording**: Adds new notification timestamps when sent

### Key Features

- **Sliding Window**: Smooth rate limiting without burst allowances
- **Automatic Cleanup**: Removes expired notifications automatically
- **Memory Efficient**: Bounded memory usage with time-based expiration
- **Thread Safe**: Designed for single-threaded use with mutable references

### Usage

```rust
use eyes::alerts::RateLimiter;

let mut limiter = RateLimiter::new(5); // 5 notifications per minute

if limiter.can_send() {
    // Send notification
    send_notification();
    limiter.record_notification();
} else {
    println!("Rate limited - too many recent notifications");
}
```

## Notification Format

### Title Format
```
System Alert: {insight_summary}
```

### Body Format
```
Cause: {root_cause}

Recommendations:
1. {first_recommendation}
2. {second_recommendation}
3. {third_recommendation}
... and N more recommendations
```

### Content Rules

- **Root Cause**: Included if available, omitted if None
- **Recommendations**: Up to 3 shown in detail
- **Overflow Handling**: Indicates additional recommendations if more than 3
- **Empty Handling**: Gracefully handles insights with no recommendations
- **UTF-8 Safety**: Text truncation respects character boundaries to prevent corruption
- **Length Limits**: Title limited to 256 characters, body to 1024 characters

## Severity Filtering

By default, only critical insights trigger notifications:

- **Critical**: Always sent (subject to rate limiting)
- **Warning**: Logged but not sent as notifications
- **Info**: Logged but not sent as notifications

This prevents notification fatigue while ensuring critical issues get immediate attention.

## Error Handling

### Rate Limiting and Queueing
- **Behavior**: Rate-limited notifications are queued instead of dropped
- **Queue Overflow**: When queue is full, oldest alerts are dropped with warning logs
- **Recovery**: Rate limit resets as time window slides, allowing queue processing
- **Monitoring**: Current count available via `current_notification_count()` and `queued_alert_count()`

### Notification Failures
- **osascript Errors**: Logged but don't halt system operation
- **Permission Issues**: Gracefully handled with error messages
- **System Unavailable**: Continues monitoring even if notifications fail

### Graceful Degradation
- **No Permissions**: System continues monitoring without notifications
- **osascript Missing**: Rare but handled gracefully
- **Rate Limit Exceeded**: Drops notifications but continues processing

## macOS Integration

### Notification Delivery
Uses AppleScript via osascript for authentic macOS notifications:

```applescript
display notification "notification_body" with title "notification_title"
```

### Permissions
- **Automatic Request**: macOS prompts for notification permission on first use
- **User Control**: Users can disable notifications in System Preferences
- **Graceful Handling**: System continues operation if permission denied

### Notification Center
- **Native Appearance**: Notifications appear in macOS Notification Center
- **User Preferences**: Respects user's notification settings (Do Not Disturb, etc.)
- **Persistence**: Notifications persist in Notification Center until dismissed

## Testing

### Unit Tests
- **Rate Limiting Logic**: Verifies sliding window behavior
- **Content Formatting**: Ensures proper notification formatting
- **Error Handling**: Tests graceful failure scenarios

### Property-Based Tests
- **Rate Limit Compliance**: Verifies rate limiting prevents spam
- **Content Completeness**: Ensures all insight data is properly formatted
- **Failure Resilience**: Tests continued operation after failures

### Integration Considerations
- **osascript Dependency**: Tests mock osascript execution
- **Time-Based Logic**: Uses controlled timestamps for deterministic testing
- **Permission Simulation**: Tests behavior with and without notification permissions

## Performance

### Memory Usage
- **Bounded Growth**: Rate limiter automatically cleans up old entries
- **Efficient Storage**: Uses VecDeque for O(1) insertion and removal
- **Minimal Overhead**: Only stores timestamps, not full notification content
- **Thread Safety**: Arc/Mutex overhead is minimal for shared state access

### CPU Usage
- **Lazy Cleanup**: Cleanup only occurs when needed
- **Efficient Filtering**: Uses iterator methods for timestamp filtering
- **Non-Blocking**: Notification delivery doesn't block system monitoring

### Concurrency
- **Thread Safe**: Safe for concurrent access across multiple threads
- **Lock Contention**: Minimal lock holding time for good performance

## Monitoring and Debugging

### Self-Monitoring Integration

The AlertManager automatically tracks notification system performance:

- **Delivery Success Rate**: Percentage of successful notifications
- **Delivery Failure Rate**: Count of failed notification attempts
- **Performance Metrics**: Integration with application-wide self-monitoring system
- **Automatic Warnings**: Alerts when notification success rate drops below 90%

See [Self-Monitoring](self-monitoring.md) for complete details on metrics collection and analysis.

### Debug Logging
Enable detailed logging for troubleshooting:

```bash
# Enable debug logging (via environment variable)
RUST_LOG=debug cargo run

# Enable debug logging (via CLI flag)
cargo run -- --verbose
```

Shows:
- Rate limiting decisions
- Notification formatting
- osascript execution results
- Error details and recovery

### Metrics
Available through AlertManager methods:
- `current_notification_count()`: Current rate limit usage
- `can_send_notification()`: Whether rate limit allows sending

### Common Issues

**No notifications appearing:**
- Check macOS notification permissions
- Verify osascript is available
- Check rate limiting isn't blocking all notifications

**Too many notifications:**
- Reduce `rate_limit_per_minute` in configuration
- Check trigger rules aren't too sensitive
- Verify AI insights have appropriate severity levels

**Notification content truncated:**
- macOS has limits on notification length
- Content is automatically formatted to fit
- Full details available in application logs
