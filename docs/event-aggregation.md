# Event Aggregation

The Event Aggregator maintains rolling buffers of recent system events, providing time-windowed access to logs and metrics for AI analysis.

## Purpose

The aggregator serves as a bridge between real-time data collection and batch AI analysis:

- **Temporal Context**: Stores recent events so AI can analyze patterns over time
- **Memory Management**: Enforces capacity limits to prevent unbounded growth
- **Efficient Queries**: Provides fast time-based filtering for trigger evaluation
- **Decoupling**: Separates collection rate from analysis rate

## Implementation

### EventAggregator

The core structure maintains two independent buffers:

```rust
pub struct EventAggregator {
    log_buffer: VecDeque<LogEvent>,
    metrics_buffer: VecDeque<MetricsEvent>,
    max_age: Duration,
    max_size: usize,
}
```

**Key Properties:**
- Uses `VecDeque` for efficient FIFO operations
- Separate buffers for logs and metrics (different arrival rates)
- Configurable time window and capacity limits
- Automatic pruning on every insert

### Buffer Management

Two complementary strategies prevent unbounded growth:

#### Time-Based Expiration

Events older than `max_age` are automatically removed:

```rust
// Default: 60 second window
let aggregator = EventAggregator::new(Duration::seconds(60), 1000);
```

Pruning occurs on every insert, removing expired events from the front of the queue.

#### Capacity Limits

When buffer size exceeds `max_size`, oldest events are dropped:

```rust
// Maximum 1000 events per buffer
let aggregator = EventAggregator::new(Duration::seconds(60), 1000);
```

This ensures memory usage stays bounded even during event bursts.

## API

### Adding Events

```rust
// Add a log event
aggregator.add_log(log_event);

// Add a metrics event
aggregator.add_metric(metrics_event);
```

Both methods automatically:
1. Append the event to the buffer
2. Enforce capacity limits
3. Prune expired entries

### Querying Events

Retrieve events within a time window:

```rust
// Get logs from the last 30 seconds
let recent_logs = aggregator.get_recent_logs(Duration::seconds(30));

// Get metrics from the last 60 seconds
let recent_metrics = aggregator.get_recent_metrics(Duration::seconds(60));
```

Returns references to events, avoiding unnecessary clones for read-only access.

### Manual Pruning

Explicit pruning is rarely needed (automatic on insert), but available:

```rust
aggregator.prune_old_entries();
```

## Configuration

The aggregator is configured via the main config file:

```toml
# Maximum age of events in the rolling buffer (seconds)
buffer_max_age_secs = 60

# Maximum number of events per buffer
buffer_max_size = 1000
```

**Tuning Guidelines:**

- **High-frequency logs**: Increase `buffer_max_size` to avoid dropping events
- **Long analysis windows**: Increase `buffer_max_age_secs` to retain more history
- **Memory-constrained systems**: Decrease both values to reduce footprint
- **Typical values**: 60s window, 1000 events handles most workloads

## Performance Characteristics

### Time Complexity

- **Insert**: O(1) amortized (push_back + potential prune)
- **Query**: O(n) where n = buffer size (linear scan with filter)
- **Prune**: O(k) where k = number of expired events

### Space Complexity

- **Memory**: O(max_size) per buffer
- **Typical footprint**: ~1MB for 1000 events (depends on message size)

### Concurrency

The aggregator is **not thread-safe** by design:
- Single-threaded access from the analysis thread
- Collectors send events via channels, not direct access
- Simplifies implementation and avoids lock contention

## Design Decisions

### Why VecDeque?

`VecDeque` provides optimal performance for this use case:
- O(1) push_back for inserts
- O(1) pop_front for pruning
- Contiguous memory for cache-friendly iteration
- No reallocation when capacity is pre-allocated

### Why Separate Buffers?

Logs and metrics have different characteristics:
- **Arrival rate**: Logs are bursty, metrics are periodic
- **Query patterns**: Often queried independently
- **Retention needs**: May want different time windows in the future

### Why References in Queries?

`get_recent_logs` returns `Vec<&LogEvent>` instead of `Vec<LogEvent>`:
- Avoids cloning potentially large messages
- Sufficient for read-only trigger evaluation
- Caller can clone if ownership is needed

## Testing

The aggregator includes comprehensive property-based tests:

### Property 6: Time-Based Expiration

Verifies that all returned events fall within the requested time window:

```rust
#[quickcheck]
fn prop_rolling_buffer_time_based_expiration(
    offsets: TimeOffsets,
    query_window: QueryWindow,
) -> bool
```

**Validates**: Requirements 3.1 (time-based expiration)

### Property 7: Capacity Enforcement

Verifies that buffer size never exceeds `max_size` and FIFO ordering is maintained:

```rust
#[quickcheck]
fn prop_rolling_buffer_enforces_capacity_limits(
    capacity: BufferCapacity,
    event_count: EventCount,
) -> bool
```

**Validates**: Requirements 3.2 (capacity limits)

## Usage Example

```rust
use macos_system_observer::aggregator::EventAggregator;
use chrono::Duration;

// Create aggregator with 60s window, 1000 event capacity
let mut aggregator = EventAggregator::new(Duration::seconds(60), 1000);

// Add events as they arrive
aggregator.add_log(log_event);
aggregator.add_metric(metrics_event);

// Query for recent errors (last 10 seconds)
let recent_errors = aggregator
    .get_recent_logs(Duration::seconds(10))
    .iter()
    .filter(|e| e.message_type == MessageType::Error)
    .collect::<Vec<_>>();

// Check if error threshold exceeded
if recent_errors.len() >= 5 {
    // Trigger AI analysis
}
```

## Future Enhancements

Potential improvements for future iterations:

- **Indexed queries**: Add secondary indices for common filters (subsystem, process)
- **Compression**: Compress old events before expiration to extend time window
- **Persistence**: Optional disk backing for post-mortem analysis
- **Statistics**: Track buffer utilization metrics for tuning
