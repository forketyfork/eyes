# I/O Handling and Shutdown Behavior

Eyes implements careful I/O handling to ensure responsive shutdown and robust data processing, particularly in the log collector component.

## Non-blocking I/O Strategy

### Problem

The original implementation used `BufReader::lines()` which could block indefinitely during shutdown, causing the application to hang when trying to terminate gracefully.

### Solution

The log collector now uses non-blocking I/O with manual buffer management:

```rust
// Set stdout to non-blocking mode
#[cfg(unix)]
{
    use std::os::unix::io::AsRawFd;
    let fd = stdout.as_raw_fd();
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}
```

### Benefits

1. **Responsive Shutdown**: The collector checks the shutdown flag before each read operation
2. **No Hanging**: Non-blocking reads with `WouldBlock` error handling prevent indefinite blocking
3. **Graceful Degradation**: Continues processing even when data arrives in partial chunks
4. **Resource Efficiency**: 10ms sleep intervals when no data is available

## Manual Line Buffering

### Implementation

Instead of relying on `BufReader::lines()`, the collector implements manual line buffering:

```rust
let mut buffer = String::new();
let mut temp_buf = [0u8; 4096];

loop {
    match stdout.read(&mut temp_buf) {
        Ok(n) => {
            let chunk = String::from_utf8_lossy(&temp_buf[..n]);
            buffer.push_str(&chunk);
            
            // Process complete lines
            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].to_string();
                buffer.drain(..=newline_pos);
                // Process line...
            }
        }
        // Error handling...
    }
}
```

### Advantages

1. **Partial Line Handling**: Incomplete lines are buffered until complete
2. **Efficient Processing**: 4KB read chunks balance memory usage and system call overhead
3. **UTF-8 Safety**: Uses `from_utf8_lossy` to handle invalid UTF-8 sequences gracefully
4. **Memory Management**: Drains processed lines from buffer to prevent unbounded growth

## Shutdown Coordination

### Thread-Safe Signaling

The collector uses `Arc<Mutex<bool>>` for shutdown coordination:

```rust
// Check shutdown flag before each operation
if !*running.lock().unwrap() {
    debug!("Stopping log processing due to shutdown signal");
    break;
}
```

### Graceful Termination

1. **Main Thread**: Sets running flag to false
2. **Collector Thread**: Detects flag change and exits processing loop
3. **Process Cleanup**: Kills child process and waits for termination
4. **Channel Cleanup**: Detects closed channels and terminates gracefully

## Error Handling Patterns

### Non-blocking Read Errors

```rust
match stdout.read(&mut temp_buf) {
    Ok(0) => break,  // EOF
    Ok(n) => { /* process data */ }
    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
        std::thread::sleep(Duration::from_millis(10));
        continue;
    }
    Err(e) => return Err(CollectorError::IoError(e)),
}
```

### Channel Closure Detection

```rust
if let Err(e) = channel.send(event) {
    warn!("Failed to send log event to channel: {}", e);
    return Ok(());  // Graceful exit on channel closure
}
```

## Performance Characteristics

- **Read Buffer Size**: 4KB chunks balance system call overhead with memory usage
- **Sleep Interval**: 10ms when no data available prevents busy-waiting
- **Memory Bounds**: Line buffer grows only with incomplete lines, not total data volume
- **CPU Usage**: Minimal overhead from shutdown flag checks and non-blocking operations

This approach ensures Eyes can handle high-volume log streams while maintaining responsive shutdown behavior and robust error recovery.