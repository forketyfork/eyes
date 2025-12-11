# Buffer Parsing and Stream Processing

Eyes processes continuous streams of data from macOS system tools, requiring robust buffer management to handle partial reads, malformed data, and mixed content scenarios.

## Overview

Both the Log Collector and Metrics Collector implement sophisticated buffer parsing to handle real-world streaming data challenges:

- **Partial reads**: Data may arrive in chunks that split JSON objects across read boundaries
- **Malformed entries**: Invalid JSON or corrupted data should not halt processing
- **Mixed content**: Valid and invalid entries may be interleaved in the stream
- **Memory efficiency**: Buffers must be managed to prevent unbounded growth

## Buffer Parsing Strategy

### Line-Based Processing

Both collectors use line-based parsing where each line represents a complete JSON object:

```rust
// Process complete lines from buffer
while let Some(newline_pos) = buffer.find('\n') {
    let line = buffer[..newline_pos].to_string();
    buffer.drain(..=newline_pos);
    
    // Parse individual line as JSON
    match Event::from_json(&line) {
        Ok(event) => { /* Process valid event */ }
        Err(_) => { /* Skip malformed entry gracefully */ }
    }
}
```

### Incomplete Line Handling

The metrics collector implements sophisticated logic to handle incomplete lines that may span multiple read operations:

```rust
// Check if last line is incomplete (no trailing newline)
if i == lines.len() - 1 && !buffer_str.ends_with('\n') {
    // This line might be incomplete, keep it in buffer
    break;
} else {
    // This is a complete but malformed line, skip it
    parsed_lines = i + 1;
}
```

### Buffer State Management

After processing, the buffer is updated to retain only unparsed content:

```rust
// Remove successfully parsed lines from buffer
let remaining_lines: Vec<&str> = lines.into_iter().skip(parsed_lines).collect();
let remaining_content = remaining_lines.join("\n");

// Preserve newline structure for incomplete lines
let new_buffer_content = if !remaining_content.is_empty() && 
                            buffer_str.ends_with('\n') && 
                            parsed_lines > 0 {
    remaining_content + "\n"
} else {
    remaining_content
};

*buffer = new_buffer_content.into_bytes();
```

## Error Handling Strategies

### Graceful Degradation

Malformed entries are logged but do not interrupt processing:

```rust
Err(e) => {
    debug!("Failed to parse entry '{}': {}", line, e);
    // Continue processing next line
}
```

### Memory Protection

Buffers are bounded to prevent memory exhaustion:

- **Line limits**: Very long lines are truncated with indicators
- **Buffer size**: Total buffer size is monitored and limited
- **Cleanup**: Successfully parsed content is immediately removed

## Format Support

### JSON Lines (Fallback Format)

Used by the fallback monitoring system when powermetrics is unavailable:

```json
{"timestamp": "2024-12-09T18:30:45.123456Z", "cpu_power_mw": 0.0, "gpu_power_mw": null, "memory_pressure": "Normal"}
{"timestamp": "2024-12-09T18:30:50.123456Z", "cpu_power_mw": 0.0, "gpu_power_mw": null, "memory_pressure": "Warning"}
```

### Plist Format (PowerMetrics)

Native powermetrics output in Apple's property list format:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>timestamp</key>
    <string>2024-12-09T18:30:45.123456Z</string>
    <!-- Additional metrics data -->
</dict>
</plist>
```

## Testing Strategy

### Property-Based Testing

Buffer parsing is validated using property-based tests that generate:

- **Valid data**: Ensures all valid entries are parsed correctly
- **Malformed data**: Verifies graceful handling of invalid JSON
- **Mixed content**: Tests interleaved valid and invalid entries
- **Split scenarios**: Validates handling of data split across reads

### Edge Cases

Specific test cases cover:

- Empty buffers and lines
- Very long lines that exceed reasonable limits
- Unicode and special characters
- Binary data that produces invalid UTF-8
- Incomplete JSON objects split across reads

## Performance Considerations

### Non-Blocking I/O

All buffer operations use non-blocking I/O to ensure responsive shutdown:

```rust
match stdout.read(&mut temp_buf) {
    Ok(0) => break, // EOF
    Ok(n) => { /* Process data */ }
    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
        std::thread::sleep(Duration::from_millis(10));
        continue;
    }
    Err(e) => return Err(e),
}
```

### Memory Efficiency

- **Streaming processing**: Data is processed as it arrives, not buffered entirely
- **Incremental parsing**: Only complete lines are parsed, partial data remains buffered
- **Bounded growth**: Buffer size is monitored and limited to prevent memory leaks

## Debugging Buffer Issues

### Common Symptoms

**Missing events**: Check for malformed JSON in logs
```bash
RUST_LOG=debug cargo run 2>&1 | grep "Failed to parse"
```

**High memory usage**: Monitor buffer growth
```bash
# Look for buffer size warnings in logs
RUST_LOG=debug cargo run 2>&1 | grep -i buffer
```

**Parsing errors**: Validate JSON format manually
```bash
# Test JSON parsing with sample data
echo '{"test": "data"}' | jq .
```

### Debug Logging

Enable debug logging to see detailed buffer operations:

```bash
RUST_LOG=debug cargo run
```

This will show:
- Buffer content before and after parsing
- Individual line parsing results
- Error details for malformed entries
- Buffer state transitions