use crate::error::CollectorError;
use crate::events::DiskEvent;
use crate::monitoring::SelfMonitoringCollector;
use chrono::Utc;
use log::{debug, error, info, warn};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Disk/filesystem collector for macOS disk activity monitoring
///
/// Monitors disk I/O activity and filesystem events using native macOS tools.
/// Uses `fs_usage` to track filesystem operations and `iostat` for disk I/O metrics.
pub struct DiskCollector {
    /// Base sampling interval for disk metrics collection
    base_sample_interval: Duration,
    /// Current adaptive sampling interval
    current_sample_interval: Arc<Mutex<Duration>>,
    /// Channel to send parsed disk events
    output_channel: Sender<DiskEvent>,
    /// Handle to the background thread
    thread_handle: Option<JoinHandle<()>>,
    /// Optional handle to the fs_usage thread
    fs_thread_handle: Option<JoinHandle<()>>,
    /// Shared state for controlling the collector
    running: Arc<Mutex<bool>>,
    /// Self-monitoring collector for resource pressure detection
    monitoring: Option<Arc<SelfMonitoringCollector>>,
}

impl DiskCollector {
    /// Create a new DiskCollector with the specified sampling interval
    ///
    /// # Arguments
    ///
    /// * `interval` - How often to sample disk metrics (e.g., Duration::from_secs(5))
    /// * `channel` - Channel to send parsed DiskEvent structures
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use std::sync::mpsc;
    /// use std::time::Duration;
    /// use eyes::collectors::DiskCollector;
    ///
    /// let (tx, rx) = mpsc::channel();
    /// let collector = DiskCollector::new(Duration::from_secs(5), tx);
    /// ```
    pub fn new(interval: Duration, channel: Sender<DiskEvent>) -> Self {
        Self {
            base_sample_interval: interval,
            current_sample_interval: Arc::new(Mutex::new(interval)),
            output_channel: channel,
            thread_handle: None,
            fs_thread_handle: None,
            running: Arc::new(Mutex::new(false)),
            monitoring: None,
        }
    }

    /// Set the self-monitoring collector for resource pressure detection
    pub fn set_monitoring(&mut self, monitoring: Arc<SelfMonitoringCollector>) {
        self.monitoring = Some(monitoring);
    }

    /// Start the disk collector
    ///
    /// Spawns a background thread that manages disk monitoring subprocesses.
    /// Uses `fs_usage` for filesystem events and `iostat` for disk I/O metrics.
    ///
    /// # Errors
    ///
    /// Returns `CollectorError::SubprocessSpawn` if disk monitoring tools cannot be started.
    pub fn start(&mut self) -> Result<(), CollectorError> {
        info!(
            "Starting DiskCollector with base interval: {:?}",
            self.base_sample_interval
        );

        // Set running flag
        {
            let mut running = self.running.lock().unwrap();
            if *running {
                info!("DiskCollector already running, skipping start");
                return Ok(()); // Already running
            }
            *running = true;
        }

        // Test that we can spawn disk monitoring tools
        debug!("Testing disk monitoring tools availability");
        let test_result = Self::test_disk_tools_availability();
        if let Err(e) = test_result {
            error!(
                "Disk monitoring tools failed to execute: {}. Continuing without disk monitoring.",
                e
            );

            // Reset running flag to indicate disk collection is not available
            {
                let mut running = self.running.lock().unwrap();
                *running = false;
            }

            return Err(CollectorError::SubprocessSpawn(format!(
                "Disk monitoring tools unavailable: {}",
                e
            )));
        } else {
            info!("Disk monitoring tools available for filesystem/disk monitoring");
        }

        let current_interval = Arc::clone(&self.current_sample_interval);
        let channel = self.output_channel.clone();
        let running = Arc::clone(&self.running);
        let monitoring = self.monitoring.clone();

        // Spawn background thread
        debug!("Spawning DiskCollector background thread");
        let handle = thread::spawn(move || {
            Self::collector_thread(current_interval, channel, running, monitoring);
        });

        // Spawn fs_usage thread (filesystem-level events) best-effort
        let fs_usage_interval = self.base_sample_interval;
        let fs_channel = self.output_channel.clone();
        let fs_running = Arc::clone(&self.running);
        let fs_handle = thread::spawn(move || {
            if let Err(e) = Self::fs_usage_thread(fs_usage_interval, fs_channel, fs_running.clone())
            {
                warn!("fs_usage monitoring disabled: {}", e);
            }
        });

        self.thread_handle = Some(handle);
        self.fs_thread_handle = Some(fs_handle);
        info!(
            "DiskCollector started successfully with base interval: {:?}",
            self.base_sample_interval
        );
        Ok(())
    }

    /// Stop the disk collector
    ///
    /// Signals the background thread to stop and waits for it to finish.
    /// This will terminate disk monitoring subprocesses gracefully.
    ///
    /// # Errors
    ///
    /// Returns `CollectorError::IoError` if there's an issue stopping the thread.
    pub fn stop(&mut self) -> Result<(), CollectorError> {
        info!("Stopping DiskCollector");

        // Set running flag to false
        {
            let mut running = self.running.lock().unwrap();
            if !*running {
                debug!("DiskCollector already stopped");
                return Ok(());
            }
            *running = false;
        }

        debug!("Signaling DiskCollector thread to stop");

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.take() {
            debug!("Waiting for DiskCollector thread to join");
            handle.join().map_err(|_| {
                error!("Failed to join DiskCollector thread");
                CollectorError::SubprocessTerminated("Failed to join collector thread".to_string())
            })?;
            debug!("DiskCollector thread joined successfully");
        }

        if let Some(fs_handle) = self.fs_thread_handle.take() {
            debug!("Waiting for fs_usage thread to join");
            let _ = fs_handle.join();
        }

        info!("DiskCollector stopped successfully");
        Ok(())
    }

    /// Test if disk monitoring tools are available on the system
    fn test_disk_tools_availability() -> Result<(), CollectorError> {
        debug!("Testing disk monitoring tools availability");

        // Test iostat (should be available without sudo)
        let iostat_output = Command::new("iostat")
            .args(["-d", "-c", "1"])
            .output()
            .map_err(|e| CollectorError::SubprocessSpawn(format!("iostat test: {}", e)))?;

        if !iostat_output.status.success() {
            return Err(CollectorError::SubprocessSpawn(
                "iostat is not available or failed".to_string(),
            ));
        }

        // Test fs_usage (requires sudo)
        let fs_usage_output = Command::new("sudo")
            .args(["-n", "fs_usage", "-h"])
            .output()
            .map_err(|e| CollectorError::SubprocessSpawn(format!("fs_usage test: {}", e)))?;

        if !fs_usage_output.status.success() {
            warn!("fs_usage requires password or is not available, will use iostat only");
        }

        Ok(())
    }

    /// Main collector thread function
    fn collector_thread(
        current_interval: Arc<Mutex<Duration>>,
        channel: Sender<DiskEvent>,
        running: Arc<Mutex<bool>>,
        monitoring: Option<Arc<SelfMonitoringCollector>>,
    ) {
        let initial_interval = *current_interval.lock().unwrap();
        info!(
            "DiskCollector thread started with initial interval: {:?}",
            initial_interval
        );

        let mut restart_delay = Duration::from_secs(1);
        let max_delay = Duration::from_secs(60);
        let mut consecutive_failures = 0;
        const MAX_CONSECUTIVE_FAILURES: u32 = 5;

        while *running.lock().unwrap() {
            // Adapt sampling frequency based on resource pressure
            if let Some(ref monitoring) = monitoring {
                let under_pressure = monitoring.is_under_resource_pressure();
                let mut current = current_interval.lock().unwrap();

                if under_pressure {
                    let new_interval = (*current).mul_f32(1.5).min(Duration::from_secs(60));
                    if new_interval != *current {
                        info!("Reducing disk sampling frequency due to resource pressure: {:?} -> {:?}", 
                              *current, new_interval);
                        *current = new_interval;
                    }
                } else {
                    let target_interval = initial_interval;
                    if *current > target_interval {
                        let new_interval = (*current).mul_f32(0.9).max(target_interval);
                        if new_interval != *current {
                            debug!("Increasing disk sampling frequency as pressure is relieved: {:?} -> {:?}", 
                                  *current, new_interval);
                            *current = new_interval;
                        }
                    }
                }
            }

            let adaptive_interval = *current_interval.lock().unwrap();

            // Try iostat first (no sudo required)
            let subprocess_result = Self::spawn_iostat(&adaptive_interval);

            match subprocess_result {
                Ok(mut child) => {
                    info!("Disk monitoring subprocess started successfully");

                    let mut had_healthy_run = false;
                    match Self::process_disk_output(&mut child, &channel, &running) {
                        Ok(_) => match child.try_wait() {
                            Ok(Some(exit_status)) => {
                                warn!("Disk subprocess exited with status: {:?}", exit_status);
                                consecutive_failures += 1;
                            }
                            Ok(None) => {
                                debug!("Disk subprocess finished normally");
                                had_healthy_run = true;
                            }
                            Err(e) => {
                                error!("Failed to check subprocess status: {}", e);
                                consecutive_failures += 1;
                            }
                        },
                        Err(e) => {
                            error!("Error processing disk output: {}", e);
                            consecutive_failures += 1;
                        }
                    }

                    if had_healthy_run {
                        consecutive_failures = 0;
                        restart_delay = Duration::from_secs(1);
                    }

                    // Clean up subprocess
                    if let Err(e) = child.kill() {
                        warn!("Failed to kill disk subprocess: {}", e);
                    }
                    let _ = child.wait();
                }
                Err(e) => {
                    error!("Failed to spawn disk subprocess: {}", e);
                    consecutive_failures += 1;
                }
            }

            if !*running.lock().unwrap() {
                break;
            }

            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                warn!(
                    "Too many consecutive failures ({}), entering degraded mode",
                    consecutive_failures
                );

                let degraded_delay = Duration::from_secs(60);
                warn!(
                    "Entering degraded mode - will retry every {:?}",
                    degraded_delay
                );

                let sleep_interval = Duration::from_millis(500);
                let mut remaining = degraded_delay;
                while remaining > Duration::ZERO && *running.lock().unwrap() {
                    let sleep_time = std::cmp::min(remaining, sleep_interval);
                    thread::sleep(sleep_time);
                    remaining = remaining.saturating_sub(sleep_time);
                }

                consecutive_failures = 0;
                restart_delay = Duration::from_secs(1);
                continue;
            }

            if consecutive_failures > 0 {
                warn!(
                    "Restarting disk collection in {:?} (failure #{}/{})",
                    restart_delay, consecutive_failures, MAX_CONSECUTIVE_FAILURES
                );
                thread::sleep(restart_delay);
                restart_delay = std::cmp::min(restart_delay * 2, max_delay);
            }
        }

        {
            let mut running_flag = running.lock().unwrap();
            *running_flag = false;
        }

        info!("Disk collector thread finished");
    }

    /// Spawn the `iostat` subprocess for disk I/O monitoring
    fn spawn_iostat(interval: &Duration) -> Result<Child, CollectorError> {
        debug!("Spawning iostat with interval: {:?}", interval);

        let interval_secs = interval.as_secs().max(1); // Minimum 1 second

        let child = Command::new("iostat")
            .args([
                "-d", // Disk statistics only
                "-c",
                &interval_secs.to_string(), // Count (run continuously)
                &interval_secs.to_string(), // Interval in seconds
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CollectorError::SubprocessSpawn(format!("iostat: {}", e)))?;

        Ok(child)
    }

    /// Spawn a best-effort fs_usage watcher to surface filesystem activity
    fn fs_usage_thread(
        interval: Duration,
        channel: Sender<DiskEvent>,
        running: Arc<Mutex<bool>>,
    ) -> Result<(), CollectorError> {
        // fs_usage requires sudo on many systems; run non-interactively and bail if unavailable
        let mut child = Command::new("sudo")
            .args([
                "-n",
                "fs_usage",
                "-w",
                "-f",
                "filesystem", // focus on filesystem events
                &interval.as_secs().max(1).to_string(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CollectorError::SubprocessSpawn(format!("fs_usage: {}", e)))?;

        // If we cannot read stdout, disable fs monitoring gracefully
        let mut stdout = match child.stdout.take() {
            Some(out) => out,
            None => {
                return Err(CollectorError::ParseError(
                    "No stdout from fs_usage".to_string(),
                ))
            }
        };

        let mut buffer = Vec::new();
        let mut temp_buf = [0u8; 4096];

        loop {
            if !*running.lock().unwrap() {
                break;
            }

            match stdout.read(&mut temp_buf) {
                Ok(0) => break,
                Ok(n) => {
                    buffer.extend_from_slice(&temp_buf[..n]);

                    if let Some(events) = Self::try_parse_fs_usage_buffer(&mut buffer) {
                        for event in events {
                            if let Err(e) = channel.send(event) {
                                if !*running.lock().unwrap() {
                                    debug!("fs_usage channel closed during shutdown");
                                } else {
                                    warn!("Failed to send fs_usage event: {}", e);
                                }
                                return Ok(());
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(e) => return Err(CollectorError::IoError(e)),
            }
        }

        let _ = child.kill();
        let _ = child.wait();
        Ok(())
    }

    /// Process output from the disk monitoring subprocess
    fn process_disk_output(
        child: &mut Child,
        channel: &Sender<DiskEvent>,
        running: &Arc<Mutex<bool>>,
    ) -> Result<(), CollectorError> {
        use std::io::Read;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| CollectorError::ParseError("No stdout available".to_string()))?;

        let mut buffer = Vec::new();
        let mut temp_buf = [0u8; 4096];

        loop {
            if !*running.lock().unwrap() {
                debug!("Stopping disk processing due to shutdown signal");
                break;
            }

            match stdout.read(&mut temp_buf) {
                Ok(0) => {
                    debug!("Disk subprocess closed stdout");
                    break;
                }
                Ok(n) => {
                    buffer.extend_from_slice(&temp_buf[..n]);

                    if let Some(parsed_events) = Self::try_parse_iostat_buffer(&mut buffer) {
                        for event in parsed_events {
                            debug!(
                                "Parsed disk event: {} - Read: {:.1} KB/s, Write: {:.1} KB/s",
                                event.disk_name, event.read_kb_per_sec, event.write_kb_per_sec
                            );

                            if let Err(e) = channel.send(event) {
                                if !*running.lock().unwrap() {
                                    debug!("Disk channel closed during shutdown");
                                } else {
                                    warn!("Failed to send disk event to channel: {}", e);
                                }
                                return Ok(());
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(e) => {
                    return Err(CollectorError::IoError(e));
                }
            }
        }

        Ok(())
    }

    /// Try to parse iostat output from the buffer
    fn try_parse_iostat_buffer(buffer: &mut Vec<u8>) -> Option<Vec<DiskEvent>> {
        let buffer_str = String::from_utf8_lossy(buffer);
        let lines: Vec<&str> = buffer_str.lines().collect();
        let mut events = Vec::new();
        let mut parsed_lines = 0;

        for (i, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                parsed_lines = i + 1;
                continue;
            }

            // Parse iostat output format
            // Example line: "disk0       1.23     4.56     0.12     0.34"
            if let Ok(event) = DiskEvent::from_iostat_line(line) {
                events.push(event);
                parsed_lines = i + 1;
            } else {
                // If this is the last line and buffer doesn't end with newline, might be incomplete
                if i == lines.len() - 1 && !buffer_str.ends_with('\n') {
                    break;
                } else {
                    // Skip malformed line
                    parsed_lines = i + 1;
                }
            }
        }

        if parsed_lines > 0 {
            let remaining_lines: Vec<&str> = lines.into_iter().skip(parsed_lines).collect();
            let remaining_content = remaining_lines.join("\n");
            let new_buffer_content = if !remaining_content.is_empty()
                && buffer_str.ends_with('\n')
                && parsed_lines > 0
            {
                remaining_content + "\n"
            } else {
                remaining_content
            };

            *buffer = new_buffer_content.into_bytes();

            if !events.is_empty() {
                Some(events)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Parse a single fs_usage line into a DiskEvent (best-effort)
    fn parse_fs_usage_line(line: &str) -> Option<DiskEvent> {
        // fs_usage lines are noisy; we look for read/write ops and a path near the end
        // Example (simplified): "12:00:00.000  read /Users/test/file.txt 1024 bytes"
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 4 {
            return None;
        }

        let is_read = tokens.iter().any(|t| t.eq_ignore_ascii_case("read"));
        let is_write = tokens.iter().any(|t| t.eq_ignore_ascii_case("write"));
        if !is_read && !is_write {
            return None;
        }

        // Heuristic: last token that looks like a path (starts with /)
        let filesystem_path = tokens
            .iter()
            .rev()
            .find(|t| t.starts_with('/'))
            .map(|s| s.to_string());

        // Try to find a byte count
        let mut bytes: f64 = 0.0;
        for token in tokens.iter().rev() {
            if let Ok(val) = token.parse::<f64>() {
                bytes = val;
                break;
            }
        }

        let kb = bytes / 1024.0;
        let (read_kb_per_sec, write_kb_per_sec, read_ops_per_sec, write_ops_per_sec) =
            if is_read && !is_write {
                (kb, 0.0, 1.0, 0.0)
            } else if is_write && !is_read {
                (0.0, kb, 0.0, 1.0)
            } else {
                // Both mentioned; split evenly
                (kb / 2.0, kb / 2.0, 1.0, 1.0)
            };

        Some(DiskEvent {
            timestamp: Utc::now(),
            read_kb_per_sec,
            write_kb_per_sec,
            read_ops_per_sec,
            write_ops_per_sec,
            disk_name: "fs_usage".to_string(),
            filesystem_path,
        })
    }

    /// Parse fs_usage output buffer into DiskEvents
    fn try_parse_fs_usage_buffer(buffer: &mut Vec<u8>) -> Option<Vec<DiskEvent>> {
        let buffer_str = String::from_utf8_lossy(buffer);
        let lines: Vec<&str> = buffer_str.lines().collect();
        let mut events = Vec::new();
        let mut parsed_lines = 0;

        for (i, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                parsed_lines = i + 1;
                continue;
            }

            if let Some(event) = Self::parse_fs_usage_line(line) {
                events.push(event);
                parsed_lines = i + 1;
            } else if i == lines.len() - 1 && !buffer_str.ends_with('\n') {
                break;
            } else {
                parsed_lines = i + 1;
            }
        }

        if parsed_lines > 0 {
            let remaining_lines: Vec<&str> = lines.into_iter().skip(parsed_lines).collect();
            let remaining_content = remaining_lines.join("\n");
            let new_buffer_content = if !remaining_content.is_empty()
                && buffer_str.ends_with('\n')
                && parsed_lines > 0
            {
                remaining_content + "\n"
            } else {
                remaining_content
            };

            *buffer = new_buffer_content.into_bytes();

            if !events.is_empty() {
                Some(events)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Check if the collector is currently running
    pub fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }
}

impl Drop for DiskCollector {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn test_disk_collector_creation() {
        let (tx, _rx) = mpsc::channel();
        let collector = DiskCollector::new(Duration::from_secs(5), tx);
        assert!(!collector.is_running());
        assert_eq!(collector.base_sample_interval, Duration::from_secs(5));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_disk_collector_start_stop() {
        let (tx, _rx) = mpsc::channel();
        let mut collector = DiskCollector::new(Duration::from_secs(5), tx);

        let start_result = collector.start();
        match start_result {
            Ok(_) => {
                assert!(collector.is_running());
                assert!(collector.stop().is_ok());
                assert!(!collector.is_running());
            }
            Err(_) => {
                assert!(!collector.is_running());
            }
        }
    }

    #[test]
    fn test_disk_tools_availability_check() {
        let result = DiskCollector::test_disk_tools_availability();
        // Should either succeed or fail gracefully
        match result {
            Ok(_) => {
                // Tools are available
            }
            Err(CollectorError::SubprocessSpawn(_)) => {
                // Tools are not available, which is acceptable
            }
            Err(e) => {
                panic!("Unexpected error type: {:?}", e);
            }
        }
    }

    #[test]
    fn test_parse_fs_usage_line_basic() {
        let line = "12:00:00.000 read /Users/test/file.txt 2048 bytes";
        let event = DiskCollector::parse_fs_usage_line(line).unwrap();
        assert!(event.read_kb_per_sec > 0.0);
        assert_eq!(event.write_kb_per_sec, 0.0);
        assert_eq!(
            event.filesystem_path,
            Some("/Users/test/file.txt".to_string())
        );
        assert_eq!(event.disk_name, "fs_usage");
    }

    #[test]
    fn test_parse_fs_usage_line_write() {
        let line = "12:00:00.000 WRITE /var/log/system.log 1024";
        let event = DiskCollector::parse_fs_usage_line(line).unwrap();
        assert_eq!(event.read_kb_per_sec, 0.0);
        assert!(event.write_kb_per_sec > 0.0);
        assert_eq!(
            event.filesystem_path,
            Some("/var/log/system.log".to_string())
        );
    }
}
