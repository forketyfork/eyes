# macOS Integration

Eyes integrates deeply with macOS system tools and APIs.

## Unified Logging System

Eyes uses the `log stream` command to access macOS's native logging infrastructure. The `LogCollector` automatically manages this subprocess with robust error recovery.

### Command Usage

The collector spawns this command automatically:

```bash
log stream --predicate 'messageType == error OR messageType == fault' --style json
```

### Output Format

JSON arrays with log entries:

```json
[
  {
    "timestamp": "2024-12-09 10:30:45.123456-0800",
    "messageType": "Error",
    "subsystem": "com.apple.Safari",
    "category": "WebProcess",
    "process": "Safari",
    "processID": 1234,
    "message": "Failed to load resource: net::ERR_CONNECTION_REFUSED"
  }
]
```

### Predicate Syntax

Apple's predicate language for filtering:

- `messageType == error` - Only errors
- `messageType == fault` - Critical errors
- `subsystem BEGINSWITH "com.apple"` - Apple subsystems
- `process == "Safari"` - Specific process
- `message CONTAINS "memory"` - Text search

Combine with `AND`, `OR`, `NOT`:
```
messageType == error AND subsystem == "com.apple.Safari"
```

## powermetrics

Eyes uses `powermetrics` for detailed resource metrics.

### Command Usage

```bash
sudo powermetrics --samplers cpu_power,gpu_power --format plist -i 5000
```

### Permissions

Requires sudo or setuid wrapper. Falls back gracefully if unavailable.

### Output Format

Property list (plist) with nested metrics:

```xml
<dict>
  <key>processor</key>
  <dict>
    <key>cpu_power</key>
    <real>1234.5</real>
  </dict>
  <key>gpu</key>
  <dict>
    <key>gpu_power</key>
    <real>567.8</real>
  </dict>
</dict>
```

### Degraded Mode

If powermetrics unavailable:
- Continues log monitoring without metrics collection
- Provides clear error messages about reduced functionality
- Maintains system stability with limited capabilities

## Native Notifications

Eyes delivers alerts via macOS notification system.

### Implementation

Uses `osascript` to trigger notifications:

```bash
osascript -e 'display notification "Body text" with title "Title"'
```

### Notification Format

- **Title**: Issue summary from AI
- **Body**: Actionable recommendations
- **Sound**: System default (configurable)

### Permissions

Requires notification permission:
- Requested automatically on first alert
- User can grant in System Preferences → Notifications

### Rate Limiting

Default: 3 notifications per minute to prevent spam during cascading failures.

## Required Permissions

### Full Disk Access

Required to read Unified Logs:

1. Open System Preferences → Security & Privacy → Privacy
2. Select "Full Disk Access"
3. Click the lock to make changes
4. Add Eyes binary to the list

### Notification Access

Automatically requested on first notification. User can:
- Allow: Notifications appear normally
- Deny: Alerts logged but not displayed

### Sudo Access (Optional)

For enhanced metrics via powermetrics:

```bash
# Add to sudoers (use visudo)
username ALL=(ALL) NOPASSWD: /usr/bin/powermetrics
```

Or use setuid wrapper for production deployments.

## launchd Integration

Run Eyes as a background service:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.eyes.system-observer</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/eyes</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/usr/local/var/log/eyes.log</string>
    <key>StandardErrorPath</key>
    <string>/usr/local/var/log/eyes.error.log</string>
</dict>
</plist>
```

Install:
```bash
cp com.eyes.system-observer.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.eyes.system-observer.plist
```
