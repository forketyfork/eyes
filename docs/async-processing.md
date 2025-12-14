# Async Processing and Concurrency

Eyes uses a hybrid concurrency model: collectors and alert processing run on dedicated threads with blocking I/O, while AI analysis uses async/await through a shared Tokio runtime created inside the analysis thread.

## Overview

- **AI Analysis**: Non-blocking LLM backend communication executed via a single shared Tokio runtime
- **Collectors**: Blocking subprocess monitoring on dedicated threads for logs, metrics, and disk activity
- **Alert Management**: Synchronous queue processing polled by a notification thread
- **HTTP Communication**: Async requests to AI backends only

## Tokio Integration

A single Tokio runtime is created inside the analysis thread and reused for all AI calls:

```rust
let rt = tokio::runtime::Runtime::new()?;

for context in trigger_engine.evaluate(&recent_logs, &recent_metrics, &recent_disk) {
    let insight = rt.block_on(ai_analyzer.analyze(&context))?;
    // ...
}
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

Alert processing is synchronous; the notification thread polls `tick()` to drain the queue when rate limits allow:

```rust
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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

## Thread Safety

### Shared State Management

Components that need to be shared across threads use Arc/Mutex patterns:

```rust
use std::sync::{Arc, Mutex};

// Thread-safe alert manager
let alert_manager = Arc::new(Mutex::new(AlertManager::new(3)));

// Clone for use in background threads
let manager_clone = Arc::clone(&alert_manager);
std::thread::spawn(move || {
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

Async I/O is limited to HTTP requests to AI backends; collectors and alerts use blocking I/O on dedicated threads.

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

- **Analysis Thread**: Drives trigger evaluation, AI analysis (via Tokio runtime), and retry processing.
- **Notification Thread**: Calls `AlertManager::tick()` every 500ms to flush queued alerts.
- **Collectors**: Run in dedicated threads using blocking reads from subprocesses.

## Performance Considerations

### Async Overhead

- **Single Runtime**: One shared Tokio runtime inside the analysis thread avoids repeated initialization
- **Limited Scope**: Only AI HTTP calls are async; collectors and alerting remain blocking on their own threads
- **Small Footprint**: Async state for AI calls stays minimal compared to the blocking collectors

### Best Practices

- **Avoid Blocking**: Keep AI backend code non-blocking; use blocking work only on dedicated threads
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
