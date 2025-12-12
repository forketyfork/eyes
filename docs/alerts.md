# Alert System

Eyes delivers system insights through native macOS notifications with intelligent rate limiting to prevent alert fatigue. The alert system consists of two main components: the AlertManager for notification delivery and the RateLimiter for spam prevention.

## Overview

The alert system coordinates between AI-generated insights and macOS notification delivery:

1. **Insight Evaluation**: Determine if an insight warrants a notification
2. **Rate Limiting**: Check if notification frequency limits allow delivery
3. **Formatting**: Create user-friendly notification content
4. **Delivery**: Send native macOS notifications via osascript
5. **Tracking**: Record successful deliveries for rate limiting

## AlertManager

The central coordinator for notification delivery with built-in rate limiting, intelligent alert queueing, and async processing capabilities.

### Key Features

- **Severity Filtering**: Only critical insights trigger notifications by default
- **Rate Limiting**: Prevents notification spam with configurable limits
- **Alert Queueing**: Queues rate-limited alerts for later delivery when capacity becomes available
- **Queue Management**: Configurable queue size with overflow handling (drops oldest alerts)
- **Async Processing**: Background processing with tokio integration for non-blocking operations
- **Thread Safety**: Arc/Mutex-based shared state management for concurrent access
- **Native Integration**: Uses osascript for authentic macOS notifications
- **Graceful Degradation**: Continues operation even if notifications fail

### Usage

```rust
use eyes::alerts::AlertManager;
use eyes::ai::AIInsight;
use std::sync::{Arc, Mutex};

let mut alert_manager = AlertManager::new(3); // 3 notifications per minute

// Send an alert for a critical insight
match alert_manager.send_alert(&insight) {
    Ok(()) => println!("Notification sent or queued successfully"),
    Err(AlertError::NotificationFailed(msg)) => println!("Notification failed: {}", msg),
}

// Check queue status
println!("Queued alerts: {}", alert_manager.queued_alert_count());

// For async/concurrent usage with shared state
let alert_manager = Arc::new(Mutex::new(AlertManager::new(3)));
let manager_clone = Arc::clone(&alert_manager);

// Can be used across threads or async tasks
tokio::spawn(async move {
    let mut manager = manager_clone.lock().unwrap();
    manager.send_alert(&insight).unwrap();
});
```

### Configuration

```toml
[alerts]
rate_limit_per_minute = 3  # Maximum notifications per minute
```

## Alert Queueing System

The AlertManager includes intelligent alert queueing with async processing capabilities to handle rate-limited scenarios gracefully.

### Queue Behavior

- **Immediate Delivery**: Alerts are sent immediately if rate limits allow
- **Queue on Rate Limit**: When rate limited, critical alerts are queued for later processing
- **Automatic Processing**: Queued alerts are processed when rate limit capacity becomes available
- **Autonomous Processing**: The `tick()` method enables autonomous queue processing without external triggers
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
use std::time::Duration;
use tokio::time::interval;

// In your main application loop
let mut tick_interval = interval(Duration::from_secs(10)); // Check every 10 seconds

loop {
    tokio::select! {
        // Handle other application events
        _ = some_other_task() => {
            // Handle other work
        }
        
        // Autonomous alert processing
        _ = tick_interval.tick() => {
            if let Ok(processed) = alert_manager.tick() {
                if processed > 0 {
                    println!("Processed {} queued alerts", processed);
                }
            }
        }
    }
}
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
- **Async Processing**: Background processing reduces main thread blocking

### Concurrency
- **Thread Safe**: Safe for concurrent access across multiple threads
- **Lock Contention**: Minimal lock holding time for good performance
- **Async Compatible**: Works seamlessly with tokio async runtime

## Monitoring and Debugging

### Debug Logging
Enable detailed logging for troubleshooting:

```bash
RUST_LOG=debug cargo run
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