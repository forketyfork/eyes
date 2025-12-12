# Trigger Rules

Eyes uses a rule-based system to determine when AI analysis should be invoked. This document describes the built-in trigger rules and how to customize them.

## Overview

The trigger engine evaluates recent system events against a set of configurable rules. When a rule's conditions are met, it creates a trigger context that includes:

- Recent log events and metrics that contributed to the trigger
- The rule that fired and its expected severity level
- Additional context about why the trigger activated

This context is then passed to the AI analyzer for detailed analysis and insight generation.

## Built-in Rules

### ErrorFrequencyRule

Monitors the frequency of error and fault messages within a time window.

**Purpose**: Detect when applications or system components are generating excessive errors, which may indicate instability or configuration issues.

**Configuration**:
- `threshold`: Maximum number of errors allowed (default: 5)
- `window_seconds`: Time window to count errors within (default: 60 seconds)
- `severity`: Severity level when triggered (default: Warning)

**Triggers when**: Error + fault count > threshold within the time window

**Example scenarios**:
- Application repeatedly failing to connect to a service
- System daemon encountering configuration errors
- Hardware driver reporting intermittent failures

```rust
// Default: 5 errors in 60 seconds = Warning
let rule = ErrorFrequencyRule::default();

// Custom: 10 errors in 30 seconds = Critical
let rule = ErrorFrequencyRule::new(10, 30, Severity::Critical);
```

### MemoryPressureRule

Monitors system memory pressure levels from metrics events.

**Purpose**: Detect when the system is running low on available memory, which can lead to performance degradation or application termination.

**Configuration**:
- `threshold`: Minimum memory pressure level to trigger (Warning, Critical)
- `severity`: Severity level when triggered

**Triggers when**: Any recent metrics event shows memory pressure >= threshold

**Example scenarios**:
- System approaching memory exhaustion
- Memory leaks in running applications
- Insufficient RAM for current workload

```rust
// Default: Warning level memory pressure = Warning severity
let rule = MemoryPressureRule::default();

// Critical: Only trigger on critical memory pressure
let rule = MemoryPressureRule::critical();

// Custom threshold and severity
let rule = MemoryPressureRule::new(MemoryPressure::Warning, Severity::Critical);
```

### CrashDetectionRule

Scans log messages for keywords that indicate process crashes or system failures.

**Purpose**: Immediately detect when applications crash, processes terminate unexpectedly, or the system encounters serious errors.

**Configuration**:
- `crash_keywords`: List of keywords to search for in log messages
- `severity`: Severity level when triggered (default: Critical)

**Default keywords**:
- "crash", "crashed"
- "segmentation fault", "segfault"
- "kernel panic", "panic"
- "abort", "terminated unexpectedly"
- Signal names: "SIGKILL", "SIGSEGV", "SIGABRT"
- "exception", "fatal error"

**Triggers when**: Error or fault log message contains any crash keyword (case-insensitive)

**Example scenarios**:
- Application crashes due to segmentation fault
- Process terminated by system due to resource constraints
- Kernel panic or system-level failure

```rust
// Default: Common crash indicators = Critical severity
let rule = CrashDetectionRule::default();

// Custom keywords for specific monitoring
let keywords = vec!["custom_error".to_string(), "service_failure".to_string()];
let rule = CrashDetectionRule::new(keywords, Severity::Warning);
```

### ResourceSpikeRule

Detects sudden increases in CPU or GPU power consumption using a sophisticated running minimum algorithm.

**Purpose**: Identify when applications or processes suddenly consume significantly more resources, which may indicate runaway processes, infinite loops, or resource-intensive operations. Uses a running minimum approach to reliably detect upward spikes while ignoring temporary decreases.

**Configuration**:
- `cpu_spike_threshold_mw`: Minimum CPU power increase in milliwatts (default: 1000mW)
- `gpu_spike_threshold_mw`: Minimum GPU power increase in milliwatts (default: 2000mW)
- `comparison_window_seconds`: Time window for comparing usage (default: 30 seconds)
- `severity`: Severity level when triggered (default: Warning)

**Detection Algorithm**:
The rule uses a **running minimum approach** that tracks the lowest power consumption seen so far and compares each new measurement against this baseline. This method:
- Only detects upward spikes (increases in resource usage)
- Ignores temporary decreases that might confuse other algorithms
- Catches both gradual increases and sudden transient spikes
- Maintains accuracy even when resource usage fluctuates
- Captures transient spikes that return to baseline quickly

**Triggers when**: CPU or GPU power increase >= threshold compared to the running minimum within the comparison window

**Example scenarios**:
- Background process consuming excessive CPU
- Graphics-intensive application launching
- Cryptocurrency mining or similar resource-intensive tasks
- Runaway processes or infinite loops
- Transient resource spikes that return to baseline quickly

```rust
// Default: 1000mW CPU or 2000mW GPU spike in 30 seconds = Warning
let rule = ResourceSpikeRule::default();

// Custom: More sensitive thresholds
let rule = ResourceSpikeRule::new(500.0, 1000.0, 15, Severity::Critical);
```

**Advanced Spike Detection**:

The ResourceSpikeRule uses a **running minimum algorithm** for superior spike detection:

**Algorithm Details**:
1. **Initialize**: Set running minimum to the first measurement's power values
2. **For each subsequent measurement**:
   - Calculate spike = current_power - running_minimum
   - If spike > 0, update maximum spike seen
   - Update running minimum = min(running_minimum, current_power)
3. **Trigger**: If maximum spike >= threshold

**Key Advantages**:
- **Upward-only detection**: Only triggers on increases, never on decreases
- **Transient spike capture**: Catches brief spikes that return to baseline
- **Baseline tracking**: Automatically adjusts to the lowest power consumption seen
- **Robust against fluctuations**: Ignores temporary dips in resource usage

This approach provides more accurate and reliable spike detection than simple consecutive comparisons or min-max range analysis.

## Rule Evaluation

### Time Windows

Most rules use time-based windows to evaluate recent events:

- **ErrorFrequencyRule**: Counts events within `window_seconds` from now
- **MemoryPressureRule**: Evaluates all recent metrics events
- **CrashDetectionRule**: Evaluates all recent log events
- **ResourceSpikeRule**: Compares metrics within `comparison_window_seconds`

### Event Filtering

Rules filter events based on their criteria:

- **Log events**: Only Error and Fault messages are considered for error-based rules
- **Metrics events**: All metrics events are evaluated for resource and memory rules
- **Message content**: Crash detection performs case-insensitive keyword matching

### Severity Mapping

Each rule assigns a severity level when triggered:

- **Info**: General information or low-priority issues
- **Warning**: Issues that should be monitored but aren't immediately critical
- **Critical**: Issues requiring immediate attention

## Customization

### Adding Custom Rules

Implement the `TriggerRule` trait to create custom rules:

```rust
use crate::triggers::TriggerRule;
use crate::events::{LogEvent, MetricsEvent, Severity};

struct CustomRule {
    // Rule configuration
}

impl TriggerRule for CustomRule {
    fn evaluate(&self, log_events: &[LogEvent], metrics_events: &[MetricsEvent]) -> bool {
        // Custom evaluation logic
        false
    }

    fn name(&self) -> &str {
        "CustomRule"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }
}
```

### Configuring the Engine

Add rules to the trigger engine:

```rust
use crate::triggers::TriggerEngine;
use crate::triggers::rules::*;

let mut engine = TriggerEngine::new();

// Add built-in rules with custom configuration
engine.add_rule(Box::new(ErrorFrequencyRule::new(3, 30, Severity::Critical)));
engine.add_rule(Box::new(MemoryPressureRule::critical()));
engine.add_rule(Box::new(CrashDetectionRule::default()));
engine.add_rule(Box::new(ResourceSpikeRule::new(2000.0, 3000.0, 60, Severity::Warning)));

// Add custom rules
engine.add_rule(Box::new(CustomRule::new()));
```

## Performance Considerations

### Evaluation Frequency

Rules are evaluated each time the trigger engine processes events. Consider:

- **Time window size**: Larger windows require more event processing
- **Rule complexity**: Complex evaluation logic impacts performance
- **Event volume**: High-frequency events increase evaluation overhead

### Memory Usage

Rules that examine event content (like crash detection) may impact memory usage:

- **Keyword lists**: Longer keyword lists increase matching overhead
- **Event retention**: Larger time windows retain more events in memory
- **String operations**: Case-insensitive matching has computational cost

### Optimization Tips

- Use appropriate time windows for your monitoring needs
- Limit keyword lists to essential terms
- Consider rule ordering (more specific rules first)
- Monitor rule evaluation performance in high-volume environments

## Testing

Rules include comprehensive test coverage:

- **Unit tests**: Verify individual rule behavior with controlled inputs
- **Property-based tests**: Validate rule behavior across random input scenarios
- **Integration tests**: Test rule interaction with the trigger engine
- **Transient spike tests**: Specialized tests for ResourceSpikeRule's advanced detection methods

Run rule-specific tests:

```bash
# Test all trigger rules
cargo test triggers::rules

# Test specific rule
cargo test error_frequency_rule

# Test resource spike detection including transient spikes
cargo test resource_spike_rule

# Run property-based tests with more iterations
cargo test --release triggers::property_tests
```

**ResourceSpikeRule Testing**:

The ResourceSpikeRule includes comprehensive tests for the running minimum algorithm:
- `test_resource_spike_rule_transient_spike`: Verifies detection of CPU spikes that return to baseline
- `test_resource_spike_rule_transient_gpu_spike`: Verifies detection of GPU spikes that return to baseline
- `test_resource_spike_rule_mixed_up_down_pattern`: Tests complex fluctuation patterns with multiple peaks and valleys
- `test_resource_spike_rule_no_trigger_on_decrease`: Ensures decreases don't trigger false positives
- Property-based tests validate spike detection across random resource usage patterns

These tests ensure the running minimum algorithm correctly identifies upward spikes while ignoring decreases and fluctuations, providing reliable detection of both sustained increases and transient resource spikes.

## Debugging

Enable debug logging to see rule evaluation details:

```bash
# Enable debug logging (via environment variable)
RUST_LOG=debug cargo run

# Enable debug logging (via CLI flag)
cargo run -- --verbose
```

This will show:
- Which rules are being evaluated
- Rule evaluation results (triggered/not triggered)
- Event counts and time windows
- Trigger context creation

## Future Enhancements

Potential improvements to the trigger system:

- **Configuration file support**: Load rule configuration from TOML
- **Dynamic rule loading**: Add/remove rules at runtime
- **Rule dependencies**: Rules that depend on other rule states
- **Statistical rules**: Rules based on statistical analysis of metrics
- **Machine learning rules**: Rules that learn from historical patterns