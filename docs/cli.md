# Command Line Interface

Eyes provides a command-line interface built with `clap` for configuring and running the macOS System Observer.

## Usage

```bash
eyes [OPTIONS]
```

## Options

### Configuration

- `-c, --config <FILE>`: Path to configuration file (TOML format)
  - Optional: If not provided, uses built-in defaults
  - File must exist and be readable
  - Recommended extension: `.toml`
  - Example: `--config config.toml`

### Logging

- `-v, --verbose`: Enable verbose logging output
  - Sets `RUST_LOG=debug` environment variable
  - Shows detailed information about system operations
  - Useful for troubleshooting and development

### Help

- `-h, --help`: Show help information
- `--version`: Show version information

## Examples

### Basic Usage

```bash
# Run with default configuration
eyes

# Run with custom configuration file
eyes --config my-config.toml

# Run with verbose logging
eyes --verbose

# Combine options
eyes --config config.toml --verbose
```

### Configuration File Validation

The CLI automatically validates configuration file paths with graceful fallback behavior:

- **File existence**: Missing files are handled gracefully by the configuration loader (not CLI validation)
- **File type**: Verifies it's a regular file (not a directory) if it exists
- **Extension warning**: Warns if file doesn't have `.toml` extension (non-fatal)
- **UTF-8 validation**: Ensures file paths contain valid UTF-8 characters

```bash
# Valid configuration file
eyes --config config.toml

# Missing file: handled gracefully by configuration loader (not CLI validation)
eyes --config nonexistent.toml
# Output: Configuration file 'nonexistent.toml' not found or unreadable, using defaults

# Error: path is a directory (exits with code 1)
eyes --config /path/to/directory
# Output: Configuration path is not a file: /path/to/directory

# Warning: non-standard extension (but still works)
eyes --config config.txt
# Output: Configuration file does not have .toml extension: config.txt

# Error: invalid UTF-8 in path
eyes --config $'\xff\xfe\xfd'
# Output: Configuration file path contains invalid UTF-8 characters: [invalid path]
```

## Environment Variables

### Logging Control

The CLI respects standard Rust logging environment variables:

```bash
# Set log level manually (overridden by --verbose)
RUST_LOG=info eyes

# Enable debug logging for specific modules
RUST_LOG=eyes::collectors=debug eyes

# Trace level for all modules
RUST_LOG=trace eyes
```

### Configuration Precedence

1. **CLI arguments**: `--config` flag takes highest precedence
2. **Default behavior**: Uses built-in defaults if no config specified
3. **Graceful fallback**: Missing or unreadable config files fall back to defaults with warning (handled by configuration loader)
4. **Error handling**: CLI validation only fails on critical path errors (invalid UTF-8, directories); file existence is handled by the configuration system

## Signal Handling

Eyes handles system signals for graceful shutdown:

- **SIGINT (Ctrl+C)**: Initiates graceful shutdown sequence
- **SIGTERM**: Handled for daemon/service deployment (via ctrlc crate)

```bash
# Start the application
eyes --config config.toml

# Graceful shutdown (in another terminal or via Ctrl+C)
# The application will:
# 1. Stop all collectors
# 2. Process remaining events
# 3. Clean up resources
# 4. Exit cleanly
```

## Exit Codes

Eyes uses standard exit codes to indicate success or failure:

- **0**: Successful execution and clean shutdown
- **1**: Error during startup, configuration, or runtime

Common error scenarios:

```bash
# Configuration file not found (falls back to defaults)
eyes --config missing.toml
# Exit code: 0 (with warning message)

# Invalid configuration format (falls back to defaults)
eyes --config invalid.toml
# Exit code: 0 (with error message and fallback warning)

# Configuration path is a directory
eyes --config /path/to/directory
# Exit code: 1

# Invalid UTF-8 in configuration path
eyes --config $'\xff\xfe\xfd'
# Exit code: 1

# Insufficient permissions (e.g., no Full Disk Access)
eyes
# Exit code: 1 (with appropriate error message)
```

## Integration with System Services

### macOS LaunchAgent

Create a plist file for automatic startup:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.example.eyes</string>
    <key>ProgramArguments</key>
    <array>
        <string>/path/to/eyes</string>
        <string>--config</string>
        <string>/path/to/config.toml</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
```

### Shell Scripts

Wrapper script for custom deployment:

```bash
#!/bin/bash
# eyes-wrapper.sh

CONFIG_FILE="${HOME}/.config/eyes/config.toml"
LOG_FILE="${HOME}/.local/share/eyes/eyes.log"

# Ensure directories exist
mkdir -p "$(dirname "$CONFIG_FILE")"
mkdir -p "$(dirname "$LOG_FILE")"

# Run with logging
exec /usr/local/bin/eyes \
    --config "$CONFIG_FILE" \
    --verbose \
    2>&1 | tee "$LOG_FILE"
```

## Development and Debugging

### Debug Mode

```bash
# Maximum verbosity for development
RUST_LOG=trace eyes --verbose --config debug-config.toml

# Module-specific debugging
RUST_LOG=eyes::collectors=debug,eyes::ai=trace eyes --verbose

# JSON-formatted logs (if using structured logging)
RUST_LOG_FORMAT=json eyes --verbose
```

### Startup Logging

The `--verbose` flag enables detailed startup logging that shows:

**LogCollector startup sequence:**
```
INFO  Starting LogCollector with predicate: 'messageType == error OR messageType == fault'
DEBUG Testing log stream subprocess spawn capability
DEBUG Log stream subprocess test successful
DEBUG Spawning LogCollector background thread
INFO  LogCollector started successfully with predicate: 'messageType == error OR messageType == fault'
```

**MetricsCollector startup sequence:**
```
INFO  Starting MetricsCollector with interval: 5s
DEBUG Testing powermetrics availability
INFO  powermetrics available for full metrics collection
DEBUG Spawning MetricsCollector background thread
INFO  MetricsCollector started successfully with interval: 5s
```

**Error scenarios:**
```
ERROR Failed to spawn log stream subprocess during startup test: log stream: No such file or directory
WARN  powermetrics not available: sudo powermetrics requires password. Entering degraded mode (log monitoring only).
```

### Testing Configuration

```bash
# Test configuration file validity without running
eyes --config test-config.toml --help

# Dry-run mode (if implemented)
eyes --config config.toml --dry-run
```

## Troubleshooting

### Common Issues

**"Invalid arguments" error**:
- Check that config file path contains valid UTF-8 characters
- Verify file permissions if file exists
- Ensure path is not a directory
- Missing config files are handled gracefully with warnings

**"Failed to load configuration" error**:
- Validate TOML syntax in configuration file
- Check for required fields
- Review configuration documentation

**Permission denied errors**:
- Ensure Full Disk Access permission is granted
- Check sudo access for powermetrics (optional)
- Verify notification permissions

**Startup failures**:
- Use `--verbose` to see detailed startup sequence
- Look for specific error patterns in logs:
  - `Failed to spawn log stream subprocess during startup test`: Permission or binary issues
  - `LogCollector already running, skipping start`: Duplicate start attempts
  - `Testing log stream subprocess spawn capability`: Startup validation process

### Getting Help

```bash
# Show all available options
eyes --help

# Show version information
eyes --version

# Enable verbose logging for troubleshooting
eyes --verbose --config your-config.toml
```

The CLI is designed to be self-documenting and provide clear error messages to guide users toward successful configuration and operation.