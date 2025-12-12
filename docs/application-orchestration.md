# Application Orchestration

The `SystemObserver` struct serves as the main orchestrator for all system monitoring components. It manages the lifecycle of collectors, aggregators, triggers, AI analysis, and alerting systems.

## Overview

The SystemObserver coordinates the data flow between all components:

```
Configuration → SystemObserver → Component Initialization → Event Processing Pipeline
```

## SystemObserver Structure

The main application struct contains all system components:

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
    // Communication channels and thread management
}
```

## Initialization Process

### 1. Configuration Loading

The SystemObserver supports flexible configuration loading with graceful fallback:

```rust
// Load from file with fallback to defaults if file missing
let config = SystemObserver::load_config(Some("config.toml"))?;

// Use defaults only
let config = SystemObserver::load_config(None)?;
```

**Fallback Behavior:**
- If a configuration file path is provided but the file is missing or unreadable, the system logs a warning and uses default configuration
- TOML parsing errors and validation errors are still returned as errors, but the system falls back to defaults with warnings
- UTF-8 path validation ensures proper handling of international file paths
- This ensures the application can start even with missing or problematic configuration files while still catching critical configuration problems

### 2. Component Creation

Components are initialized in dependency order:

1. **Communication Channels**: MPSC channels for inter-component communication
2. **Event Aggregator**: Shared rolling buffer with configured time windows and capacity
3. **Collectors**: Log and metrics collectors with configured predicates and intervals
4. **Trigger Engine**: Rule engine with built-in trigger rules
5. **AI Analyzer**: Analysis coordinator with configured LLM backend
6. **Alert Manager**: Notification system with rate limiting

### 3. Built-in Trigger Rules

The SystemObserver automatically configures standard trigger rules:

- **ErrorFrequencyRule**: Triggers on excessive error/fault messages
- **MemoryPressureRule**: Triggers on memory pressure warnings/critical states
- **CrashDetectionRule**: Triggers on crash indicators in logs
- **ResourceSpikeRule**: Triggers on CPU/GPU power consumption spikes

### 4. Self-Monitoring Integration

The SystemObserver integrates comprehensive self-monitoring throughout the application:

- **Component Monitoring**: AI analyzer and alert manager are automatically configured with self-monitoring
- **Thread-Safe Tracking**: Self-monitoring works consistently across main and background analysis threads
- **Performance Metrics**: Tracks AI analysis latency, notification delivery rates, and system resource usage
- **Automatic Warnings**: Detects and warns about performance degradation or system issues

## AI Backend Configuration

The SystemObserver supports multiple AI backends through configuration:

### Ollama (Local)
```rust
AIBackendConfig::Ollama { endpoint, model } => {
    let backend = OllamaBackend::new(endpoint.clone(), model.clone());
    AIAnalyzer::with_backend(Arc::new(backend))
}
```

### OpenAI (Cloud)
```rust
AIBackendConfig::OpenAI { api_key, model } => {
    let backend = OpenAIBackend::new(api_key.clone(), model.clone());
    AIAnalyzer::with_backend(Arc::new(backend))
}
```

### Mock (Testing)
```rust
AIBackendConfig::Mock => {
    let backend = MockBackend::success();
    AIAnalyzer::with_backend(Arc::new(backend))
}
```

## Thread Safety

Components that need shared access use Arc/Mutex patterns:

- **EventAggregator**: `Arc<Mutex<EventAggregator>>` for concurrent access from collectors and trigger evaluation
- **AlertManager**: `Arc<Mutex<AlertManager>>` for thread-safe notification delivery and rate limiting

## Error Handling

The SystemObserver implements comprehensive error handling:

- **Configuration Errors**: Invalid TOML, missing required fields, invalid values
- **Initialization Errors**: Component creation failures, permission issues
- **Runtime Errors**: Graceful degradation when components fail

## Lifecycle Management

### Creation
```rust
let config = SystemObserver::load_config(config_path)?;
let observer = SystemObserver::new(config)?;
```

### Startup (Coming Next)
The next implementation phase will add:
- Thread spawning for collectors
- Event processing pipeline
- Graceful shutdown handling

## Configuration Integration

The SystemObserver maps configuration sections to component initialization:

- `config.logging` → LogCollector predicate
- `config.metrics` → MetricsCollector interval
- `config.buffer` → EventAggregator capacity and time windows
- `config.triggers` → TriggerEngine rule parameters
- `config.ai` → AIAnalyzer backend selection
- `config.alerts` → AlertManager rate limiting

## Future Enhancements

## Self-Monitoring Integration

The SystemObserver includes comprehensive self-monitoring capabilities:

```rust
// Get current self-monitoring metrics
let metrics = observer.get_self_monitoring_metrics();

println!("Memory usage: {}MB", metrics.memory_usage_bytes / 1024 / 1024);
println!("Log events/min: {}", metrics.log_events_per_minute);
println!("AI latency: {:.1}ms", metrics.avg_ai_analysis_latency_ms);
println!("Notification success: {:.1}%", metrics.notification_success_rate);
```

### Automatic Performance Tracking

The SystemObserver automatically tracks:

- **Memory Usage**: Application memory consumption
- **Event Processing Rates**: Log and metrics events processed per minute
- **AI Analysis Latency**: Average time for AI backend operations
- **Notification Success Rates**: Alert delivery effectiveness
- **Performance Warnings**: Automatic detection of degraded performance

### Integration Points

- **Alert Manager**: Tracks notification delivery success/failure rates
- **AI Analysis**: Records analysis latency for performance monitoring
- **Event Processing**: Counts log and metrics events processed
- **Memory Monitoring**: Tracks application memory usage over time

See [Self-Monitoring](self-monitoring.md) for complete details.

## Future Enhancements

Planned improvements for the orchestration layer:

- **Health Monitoring**: Component health checks and restart logic
- **Dynamic Reconfiguration**: Hot-reload of configuration changes
- **Plugin System**: Dynamic loading of custom trigger rules and backends

## Testing

The SystemObserver is designed for testability:

- **Mock Backends**: Use MockBackend for AI analysis testing
- **Configuration Validation**: Test various configuration scenarios
- **Component Integration**: Verify proper wiring between components
- **Error Scenarios**: Test graceful handling of initialization failures

## Example Usage

```rust
use eyes::SystemObserver;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();
    
    // Load configuration
    let config = SystemObserver::load_config(None)?;
    
    // Create system observer
    let observer = SystemObserver::new(config)?;
    
    // Start monitoring (implementation coming next)
    // observer.start()?;
    
    Ok(())
}
```

This orchestration layer provides a clean separation between configuration, initialization, and runtime operation, making the system maintainable and testable.