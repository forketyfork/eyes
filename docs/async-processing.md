# Async Processing and Concurrency

Eyes leverages Rust's async/await capabilities and the Tokio runtime to provide efficient, non-blocking system monitoring with concurrent processing capabilities.

## Overview

The application uses async processing in several key areas:

- **AI Analysis**: Non-blocking LLM backend communication
- **Alert Management**: Background notification processing and queue management
- **Collector Management**: Concurrent subprocess monitoring
- **HTTP Communication**: Async requests to AI backends

## Tokio Integration

### Runtime Configuration

Eyes uses the Tokio async runtime with full feature set:

```rust
use tokio::time::{interval, Duration, sleep};
use std::sync::{Arc, Mutex};
```

### Key Async Components

#### AI Backends

All LLM backends implement async analysis:

```rust
impl LLMBackend for OllamaBackend {
    fn analyze<'a>(
        &'a self,
        context: &'a TriggerContext,
    ) -> Pin<Box<dyn Future<Output = Result<AIInsight, AnalysisError>> + Send + 'a>> {
        Box::pin(async move {
            // Non-blocking HTTP request to Ollama
            let response = self.client.post(self.api_url())
                .json(&request)
                .send()
                .await?;
            // Process response...
        })
    }
}
```

#### Alert Manager

The AlertManager supports async processing for background queue management:

```rust
use tokio::time::interval;
use std::sync::{Arc, Mutex};

// Background queue processing
let manager = Arc::new(Mutex::new(AlertManager::new(3)));
let manager_clone = Arc::clone(&manager);

tokio::spawn(async move {
    let mut interval = interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        if let Ok(mut mgr) = manager_clone.lock() {
            let _ = mgr.process_queue();
        }
    }
});
```

## Thread Safety

### Shared State Management

Components that need to be shared across threads use Arc/Mutex patterns:

```rust
use std::sync::{Arc, Mutex};

// Thread-safe alert manager
let alert_manager = Arc::new(Mutex::new(AlertManager::new(3)));

// Clone for use in async tasks
let manager_clone = Arc::clone(&alert_manager);
tokio::spawn(async move {
    // Safe concurrent access
    let mut manager = manager_clone.lock().unwrap();
    manager.send_alert(&insight).unwrap();
});
```

### Lock Management

- **Minimal Lock Time**: Locks are held for the shortest time possible
- **Deadlock Prevention**: Consistent lock ordering prevents deadlocks
- **Error Handling**: Lock poisoning is handled gracefully

### Retry Queue Processing

The AI analyzer implements an async retry queue for handling backend failures:

```rust
// Retry queue entry with timing
struct RetryEntry {
    context: TriggerContext,
    attempt_count: u32,
    next_retry_time: Instant,
}

// Async retry processing
impl AIAnalyzer {
    pub async fn process_retry_queue(&self) -> Vec<Result<AIInsight, AnalysisError>> {
        let mut results = Vec::new();
        let now = Instant::now();
        
        // Get entries ready for retry (non-blocking)
        let ready_entries = {
            let mut queue = self.retry_queue.lock().unwrap();
            // Extract ready entries without blocking
        };
        
        // Process entries asynchronously
        for entry in ready_entries {
            match self.analyze_without_retry(&entry.context).await {
                Ok(insight) => results.push(Ok(insight)),
                Err(e) => {
                    // Re-queue with exponential backoff or give up
                }
            }
        }
        
        results
    }
}
```

**Key Features:**
- **Non-blocking Queue Access**: Minimal lock time for queue operations
- **Async Processing**: Each retry attempt is processed asynchronously
- **Exponential Backoff**: Retry delays increase exponentially (1s, 2s, 4s...)
- **Bounded Queue**: Maximum size prevents memory exhaustion
- **Thread Safety**: Safe concurrent access from multiple threads

## Async Patterns

### Non-Blocking I/O

All I/O operations use non-blocking async patterns:

```rust
// HTTP requests to AI backends
let response = self.client
    .post(url)
    .json(&request)
    .send()
    .await?;

// Timeout handling
tokio::time::timeout(Duration::from_secs(60), operation).await??;
```

### Background Processing

Long-running tasks are spawned as background tasks:

```rust
// Background queue processing
tokio::spawn(async move {
    let mut interval = interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        // Process queued items
    }
});

// AI analysis retry queue processing
tokio::spawn(async move {
    let mut retry_interval = interval(Duration::from_secs(10));
    loop {
        retry_interval.tick().await;
        let retry_results = analyzer.process_retry_queue().await;
        // Handle retry results
    }
});
```

### Graceful Shutdown

Async tasks support graceful shutdown through cancellation:

```rust
use tokio::select;
use tokio::sync::broadcast;

let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);

tokio::spawn(async move {
    loop {
        select! {
            _ = shutdown_rx.recv() => {
                // Graceful shutdown
                break;
            }
            _ = interval.tick() => {
                // Regular processing
            }
        }
    }
});
```

## Performance Considerations

### Async Overhead

- **Task Spawning**: Minimal overhead for tokio task creation
- **Context Switching**: Efficient cooperative multitasking
- **Memory Usage**: Async state machines have small memory footprint

### Concurrency Benefits

- **Non-Blocking**: System monitoring continues during AI analysis
- **Parallel Processing**: Multiple AI requests can be processed concurrently
- **Resource Efficiency**: Better CPU utilization through async I/O

### Best Practices

- **Avoid Blocking**: Never use blocking operations in async contexts
- **Timeout Handling**: Always set timeouts for external requests
- **Error Propagation**: Use `?` operator for clean error handling
- **Resource Cleanup**: Ensure proper cleanup in async destructors

## Testing Async Code

### Async Test Framework

```rust
#[tokio::test]
async fn test_async_analysis() {
    let backend = OllamaBackend::new(
        "http://localhost:11434".to_string(),
        "llama3".to_string()
    );
    
    let context = create_test_context();
    let result = backend.analyze(&context).await;
    
    assert!(result.is_ok());
}
```

### Mock Async Backends

```rust
impl LLMBackend for MockBackend {
    fn analyze<'a>(
        &'a self,
        context: &'a TriggerContext,
    ) -> Pin<Box<dyn Future<Output = Result<AIInsight, AnalysisError>> + Send + 'a>> {
        Box::pin(async move {
            // Simulate async delay
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            
            Ok(self.response.clone())
        })
    }
}
```

## Future Enhancements

### Streaming Processing

Potential for streaming AI analysis:

```rust
// Future: Streaming analysis results
async fn stream_analysis(&self, context: &TriggerContext) 
    -> impl Stream<Item = Result<PartialInsight, AnalysisError>> {
    // Stream partial results as they become available
}
```

### Reactive Processing

Event-driven processing with async streams:

```rust
use tokio_stream::{Stream, StreamExt};

// Future: Reactive event processing
async fn process_event_stream(
    mut events: impl Stream<Item = SystemEvent>
) {
    while let Some(event) = events.next().await {
        // Process events as they arrive
    }
}
```

## Debugging Async Code

### Logging

Enable async-aware logging:

```bash
# Enable async-aware logging (via environment variable)
RUST_LOG=debug,tokio=trace cargo run

# Enable verbose logging (via CLI flag)
cargo run -- --verbose
```

### Common Issues

- **Deadlocks**: Use `tokio::sync::Mutex` instead of `std::sync::Mutex` in async contexts
- **Blocking**: Avoid `std::thread::sleep` in async functions, use `tokio::time::sleep`
- **Task Leaks**: Ensure spawned tasks have proper cleanup or cancellation

### Monitoring

Track async task performance:

```rust
use tokio::task;

let handle = tokio::spawn(async move {
    // Task implementation
});

// Monitor task completion
match handle.await {
    Ok(result) => println!("Task completed: {:?}", result),
    Err(e) => println!("Task failed: {:?}", e),
}
```