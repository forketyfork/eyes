use crate::error::CollectorError;
use crate::events::MetricsEvent;
use crate::monitoring::SelfMonitoringCollector;
use log::{debug, error, info, warn};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Metrics collector for macOS system resource monitoring
///
/// Spawns and manages a `powermetrics` subprocess to continuously monitor
/// system resource consumption including CPU power, GPU power, and memory pressure.
/// Parses plist output and sends MetricsEvent structures to a channel for processing.
pub struct MetricsCollector {
    /// Base sampling interval for metrics collection
    base_sample_interval: Duration,
    /// Current adaptive sampling interval
    current_sample_interval: Arc<Mutex<Duration>>,
    /// Channel to send parsed metrics events
    output_channel: Sender<MetricsEvent>,
    /// Handle to the background thread
    thread_handle: Option<JoinHandle<()>>,
    /// Shared state for controlling the collector
    running: Arc<Mutex<bool>>,
    /// Self-monitoring collector for resource pressure detection
    monitoring: Option<Arc<SelfMonitoringCollector>>,
}

impl MetricsCollector {
    /// Create a new MetricsCollector with the specified sampling interval
    ///
    /// # Arguments
    ///
    /// * `interval` - How often to sample metrics (e.g., Duration::from_secs(5))
    /// * `channel` - Channel to send parsed MetricsEvent structures
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use std::sync::mpsc;
    /// use std::time::Duration;
    /// use eyes::collectors::MetricsCollector;
    ///
    /// let (tx, rx) = mpsc::channel();
    /// let collector = MetricsCollector::new(Duration::from_secs(5), tx);
    /// ```
    pub fn new(interval: Duration, channel: Sender<MetricsEvent>) -> Self {
        Self {
            base_sample_interval: interval,
            current_sample_interval: Arc::new(Mutex::new(interval)),
            output_channel: channel,
            thread_handle: None,
            running: Arc::new(Mutex::new(false)),
            monitoring: None,
        }
    }

    /// Set the self-monitoring collector for resource pressure detection
    pub fn set_monitoring(&mut self, monitoring: Arc<SelfMonitoringCollector>) {
        self.monitoring = Some(monitoring);
    }

    /// Adapt sampling frequency based on resource pressure
    /// Requirement 7.4: reduce sampling frequency when constrained
    pub fn adapt_sampling_frequency(&self) {
        if let Some(monitoring) = &self.monitoring {
            let under_pressure = monitoring.is_under_resource_pressure();
            let mut current_interval = self.current_sample_interval.lock().unwrap();
            
            if under_pressure {
                // Increase interval (reduce frequency) when under pressure
                let new_interval = (*current_interval).mul_f32(2.0).min(Duration::from_secs(60));
                if new_interval != *current_interval {
                    info!("Reducing metrics sampling frequency due to resource pressure: {:?} -> {:?}", 
                          *current_interval, new_interval);
                    *current_interval = new_interval;
                }
            } else {
                // Gradually return to base interval when pressure is relieved
                let target_interval = self.base_sample_interval;
                if *current_interval > target_interval {
                    let new_interval = (*current_interval).mul_f32(0.8).max(target_interval);
                    if new_interval != *current_interval {
                        info!("Increasing metrics sampling frequency as pressure is relieved: {:?} -> {:?}", 
                              *current_interval, new_interval);
                        *current_interval = new_interval;
                    }
                }
            }
        }
    }

    /// Start the metrics collector
    ///
    /// Spawns a background thread that manages the `powermetrics` subprocess.
    /// The thread will automatically restart the subprocess if it fails.
    /// Falls back to graceful degradation if powermetrics is unavailable.
    ///
    /// # Errors
    ///
    /// Returns `CollectorError::SubprocessSpawn` if powermetrics cannot be started
    /// and no fallback is available.
    pub fn start(&mut self) -> Result<(), CollectorError> {
        info!(
            "Starting MetricsCollector with base interval: {:?}",
            self.base_sample_interval
        );

        // Set running flag
        {
            let mut running = self.running.lock().unwrap();
            if *running {
                info!("MetricsCollector already running, skipping start");
                return Ok(()); // Already running
            }
            *running = true;
        }

        // Test that we can spawn powermetrics before starting the thread
        debug!("Testing powermetrics availability");
        let test_result = Self::test_powermetrics_availability();
        if let Err(e) = test_result {
            // Requirement 7.2: log the error and continue with log monitoring only
            error!("powermetrics failed to execute: {}. Continuing with log monitoring only.", e);
            
            // Reset running flag to indicate metrics collection is not available
            {
                let mut running = self.running.lock().unwrap();
                *running = false;
            }
            
            // Return error to indicate degraded mode - caller should continue with log monitoring only
            return Err(CollectorError::SubprocessSpawn(format!(
                "powermetrics unavailable, entering degraded mode (log monitoring only): {}", e
            )));
        } else {
            info!("powermetrics available for full metrics collection");
        }

        let current_interval = Arc::clone(&self.current_sample_interval);
        let channel = self.output_channel.clone();
        let running = Arc::clone(&self.running);
        let monitoring = self.monitoring.clone();

        // Spawn background thread
        debug!("Spawning MetricsCollector background thread");
        let handle = thread::spawn(move || {
            Self::collector_thread(current_interval, channel, running, monitoring);
        });

        self.thread_handle = Some(handle);
        info!(
            "MetricsCollector started successfully with base interval: {:?}",
            self.base_sample_interval
        );
        Ok(())
    }

    /// Stop the metrics collector
    ///
    /// Signals the background thread to stop and waits for it to finish.
    /// This will terminate the `powermetrics` subprocess gracefully.
    ///
    /// # Errors
    ///
    /// Returns `CollectorError::IoError` if there's an issue stopping the thread.
    pub fn stop(&mut self) -> Result<(), CollectorError> {
        info!("Stopping MetricsCollector");

        // Set running flag to false
        {
            let mut running = self.running.lock().unwrap();
            if !*running {
                debug!("MetricsCollector already stopped");
                return Ok(());
            }
            *running = false;
        }

        debug!("Signaling MetricsCollector thread to stop");

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.take() {
            debug!("Waiting for MetricsCollector thread to join");
            handle.join().map_err(|_| {
                error!("Failed to join MetricsCollector thread");
                CollectorError::SubprocessTerminated("Failed to join collector thread".to_string())
            })?;
            debug!("MetricsCollector thread joined successfully");
        }

        info!("MetricsCollector stopped successfully");
        Ok(())
    }

    /// Test if powermetrics is available on the system
    fn test_powermetrics_availability() -> Result<(), CollectorError> {
        debug!("Testing powermetrics availability");

        // Test if powermetrics exists and if we can run it with sudo non-interactively
        let output = Command::new("sudo")
            .args(["-n", "powermetrics", "--help"])
            .output()
            .map_err(|e| CollectorError::SubprocessSpawn(format!("powermetrics test: {}", e)))?;

        if !output.status.success() {
            return Err(CollectorError::SubprocessSpawn(
                "sudo powermetrics requires password or is not available".to_string(),
            ));
        }

        Ok(())
    }

    /// Test if fallback monitoring (vm_stat, top) is available
    fn test_fallback_availability() -> bool {
        // Test if we can run vm_stat for memory information
        if let Ok(output) = Command::new("vm_stat").output() {
            if output.status.success() {
                debug!("Fallback vm_stat available");
                return true;
            }
        }

        // Test if we can run top for basic system info
        if let Ok(output) = Command::new("top").args(["-l", "1", "-n", "0"]).output() {
            if output.status.success() {
                debug!("Fallback top available");
                return true;
            }
        }

        false
    }

    /// Main collector thread function
    ///
    /// Runs in a loop, spawning and monitoring the metrics collection subprocess.
    /// Automatically restarts the subprocess with exponential backoff on failure.
    /// Supports adaptive sampling based on resource pressure.
    fn collector_thread(
        current_interval: Arc<Mutex<Duration>>,
        channel: Sender<MetricsEvent>,
        running: Arc<Mutex<bool>>,
        monitoring: Option<Arc<SelfMonitoringCollector>>,
    ) {
        let initial_interval = *current_interval.lock().unwrap();
        info!(
            "MetricsCollector thread started with initial interval: {:?}",
            initial_interval
        );

        let mut restart_delay = Duration::from_secs(1);
        let max_delay = Duration::from_secs(60);
        let mut consecutive_failures = 0;
        const MAX_CONSECUTIVE_FAILURES: u32 = 5;

        debug!("MetricsCollector thread configuration: max_failures={}, initial_delay={:?}, max_delay={:?}", 
               MAX_CONSECUTIVE_FAILURES, restart_delay, max_delay);

        while *running.lock().unwrap() {
            // Adapt sampling frequency based on resource pressure (Requirement 7.4)
            if let Some(ref monitoring) = monitoring {
                let under_pressure = monitoring.is_under_resource_pressure();
                let mut current = current_interval.lock().unwrap();
                
                if under_pressure {
                    // Increase interval (reduce frequency) when under pressure
                    let new_interval = (*current).mul_f32(1.5).min(Duration::from_secs(60));
                    if new_interval != *current {
                        info!("Reducing metrics sampling frequency due to resource pressure: {:?} -> {:?}", 
                              *current, new_interval);
                        *current = new_interval;
                    }
                } else {
                    // Gradually return to base interval when pressure is relieved
                    let target_interval = initial_interval;
                    if *current > target_interval {
                        let new_interval = (*current).mul_f32(0.9).max(target_interval);
                        if new_interval != *current {
                            debug!("Increasing metrics sampling frequency as pressure is relieved: {:?} -> {:?}", 
                                  *current, new_interval);
                            *current = new_interval;
                        }
                    }
                }
            }

            // Get current adaptive interval
            let adaptive_interval = *current_interval.lock().unwrap();
            
            // Try powermetrics first, then fallback
            let subprocess_result = Self::spawn_powermetrics(&adaptive_interval)
                .or_else(|_| Self::spawn_fallback_monitoring(&adaptive_interval));

            match subprocess_result {
                Ok(mut child) => {
                    info!("Metrics collection subprocess started successfully");

                    // Process output from the subprocess
                    let mut had_healthy_run = false;
                    match Self::process_metrics_output(&mut child, &channel, &running) {
                        Ok(_) => {
                            // Check if the subprocess is still running
                            match child.try_wait() {
                                Ok(Some(exit_status)) => {
                                    warn!(
                                        "Metrics subprocess exited with status: {:?}",
                                        exit_status
                                    );
                                    consecutive_failures += 1;
                                }
                                Ok(None) => {
                                    debug!("Metrics subprocess finished normally");
                                    had_healthy_run = true;
                                }
                                Err(e) => {
                                    error!("Failed to check subprocess status: {}", e);
                                    consecutive_failures += 1;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Error processing metrics output: {}", e);
                            consecutive_failures += 1;
                        }
                    }

                    // Only reset failure count and delay after a healthy run
                    if had_healthy_run {
                        consecutive_failures = 0;
                        restart_delay = Duration::from_secs(1);
                    }

                    // Clean up subprocess
                    if let Err(e) = child.kill() {
                        warn!("Failed to kill metrics subprocess: {}", e);
                    }
                    let _ = child.wait();
                }
                Err(e) => {
                    error!("Failed to spawn metrics subprocess: {}", e);
                    consecutive_failures += 1;
                }
            }

            // Check if we should continue running
            if !*running.lock().unwrap() {
                break;
            }

            // Check for too many consecutive failures - enter degraded mode
            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                warn!(
                    "Too many consecutive failures ({}), entering degraded mode",
                    consecutive_failures
                );

                // In degraded mode, wait longer and try less frequently
                let degraded_delay = Duration::from_secs(60); // Wait 1 minute in degraded mode
                warn!(
                    "Entering degraded mode - will retry every {:?}",
                    degraded_delay
                );

                // Sleep in short intervals to allow responsive shutdown
                let sleep_interval = Duration::from_millis(500);
                let mut remaining = degraded_delay;
                while remaining > Duration::ZERO && *running.lock().unwrap() {
                    let sleep_time = std::cmp::min(remaining, sleep_interval);
                    thread::sleep(sleep_time);
                    remaining = remaining.saturating_sub(sleep_time);
                }

                // Reset failure count to give it another chance
                consecutive_failures = 0;
                restart_delay = Duration::from_secs(1);
                continue;
            }

            // Wait before restarting with exponential backoff
            if consecutive_failures > 0 {
                warn!(
                    "Restarting metrics collection in {:?} (failure #{}/{})",
                    restart_delay, consecutive_failures, MAX_CONSECUTIVE_FAILURES
                );
                thread::sleep(restart_delay);

                // Exponential backoff
                restart_delay = std::cmp::min(restart_delay * 2, max_delay);
            }
        }

        // Reset running flag when thread exits
        {
            let mut running_flag = running.lock().unwrap();
            *running_flag = false;
        }

        info!("Metrics collector thread finished");
    }

    /// Spawn the `powermetrics` subprocess
    fn spawn_powermetrics(interval: &Duration) -> Result<Child, CollectorError> {
        debug!("Spawning powermetrics with interval: {:?}", interval);

        let interval_ms = interval.as_millis() as u64;

        // First test if we can run sudo powermetrics without hanging on password prompt
        // Use -n flag to make sudo non-interactive (fail if password required)
        let test_result = Command::new("sudo")
            .args(["-n", "powermetrics", "--help"])
            .output();

        match test_result {
            Ok(output) => {
                if !output.status.success() {
                    return Err(CollectorError::SubprocessSpawn(
                        "sudo powermetrics requires password or is not available".to_string(),
                    ));
                }
            }
            Err(e) => {
                return Err(CollectorError::SubprocessSpawn(format!(
                    "Failed to test sudo powermetrics: {}",
                    e
                )));
            }
        }

        // If test passed, spawn the actual powermetrics process
        let child = Command::new("sudo")
            .args([
                "-n", // Non-interactive mode
                "powermetrics",
                "--samplers",
                "cpu_power,gpu_power,tasks", // Include tasks sampler for memory info
                "--format",
                "plist",
                "--sample-rate",
                &interval_ms.to_string(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CollectorError::SubprocessSpawn(format!("powermetrics: {}", e)))?;

        Ok(child)
    }

    /// Spawn fallback monitoring using vm_stat and other tools
    fn spawn_fallback_monitoring(interval: &Duration) -> Result<Child, CollectorError> {
        debug!("Spawning fallback monitoring with interval: {:?}", interval);

        // Use a simple shell script that runs vm_stat periodically
        let interval_secs = interval.as_secs();
        let script = format!(
            r#"
            while true; do
                # Get memory pressure from vm_stat
                FREE_PAGES=$(vm_stat | grep 'Pages free:' | awk '{{print $3}}' | tr -d '.')
                if [ "$FREE_PAGES" -lt 100000 ]; then
                    PRESSURE="Critical"
                elif [ "$FREE_PAGES" -lt 500000 ]; then
                    PRESSURE="Warning"
                else
                    PRESSURE="Normal"
                fi
                
                # Output valid JSON
                printf '{{"timestamp": "%s", "cpu_power_mw": 0.0, "gpu_power_mw": null, "memory_pressure": "%s"}}\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%6NZ)" "$PRESSURE"
                sleep {}
            done
            "#,
            interval_secs
        );

        let child = Command::new("sh")
            .args(["-c", &script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CollectorError::SubprocessSpawn(format!("fallback monitoring: {}", e)))?;

        Ok(child)
    }

    /// Process output from the metrics collection subprocess
    fn process_metrics_output(
        child: &mut Child,
        channel: &Sender<MetricsEvent>,
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
            // Check if we should stop before each read attempt
            if !*running.lock().unwrap() {
                debug!("Stopping metrics processing due to shutdown signal");
                break;
            }

            match stdout.read(&mut temp_buf) {
                Ok(0) => {
                    // EOF reached
                    debug!("Metrics subprocess closed stdout");
                    break;
                }
                Ok(n) => {
                    // Got some data, add it to buffer
                    buffer.extend_from_slice(&temp_buf[..n]);

                    // Try to parse complete plist documents or JSON lines
                    if let Some(parsed_events) = Self::try_parse_buffer(&mut buffer) {
                        for event in parsed_events {
                            debug!(
                                "Parsed metrics event: {} - CPU: {:.1}mW, GPU: {:?}mW, Memory: {:?}",
                                event.timestamp, event.cpu_power_mw, event.gpu_power_mw, event.memory_pressure
                            );

                            // Send to channel
                            if let Err(e) = channel.send(event) {
                                warn!("Failed to send metrics event to channel: {}", e);
                                // Channel is closed, probably shutting down
                                return Ok(());
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available, sleep briefly and check running flag again
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

    /// Try to parse complete documents from the buffer
    fn try_parse_buffer(buffer: &mut Vec<u8>) -> Option<Vec<MetricsEvent>> {
        let mut events = Vec::new();

        // Try to parse multiple plist documents (powermetrics format)
        let remaining_buffer = buffer.clone();
        let mut parsed_any_plist = false;

        // Look for plist document boundaries
        // Powermetrics outputs XML plists separated by newlines
        let buffer_str = String::from_utf8_lossy(&remaining_buffer);

        // Split on plist document boundaries (<?xml version="1.0" encoding="UTF-8"?>)
        let plist_parts: Vec<&str> = buffer_str
            .split("<?xml version=\"1.0\" encoding=\"UTF-8\"?>")
            .collect();

        if plist_parts.len() > 1 {
            // We have at least one complete plist document
            let mut bytes_consumed = 0;

            for (i, part) in plist_parts.iter().enumerate() {
                if i == 0 && part.trim().is_empty() {
                    // Skip empty first part
                    bytes_consumed += part.len();
                    continue;
                }

                if i == plist_parts.len() - 1 && !part.contains("</plist>") {
                    // Last part might be incomplete, keep it in buffer
                    break;
                }

                // Reconstruct the complete plist document
                let complete_plist = if i > 0 {
                    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{}", part)
                } else {
                    part.to_string()
                };

                if complete_plist.contains("</plist>") {
                    // This looks like a complete plist document
                    match MetricsEvent::from_plist(complete_plist.as_bytes()) {
                        Ok(event) => {
                            events.push(event);
                            parsed_any_plist = true;
                            bytes_consumed += if i > 0 {
                                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".len() + part.len()
                            } else {
                                part.len()
                            };
                        }
                        Err(e) => {
                            debug!("Failed to parse plist document: {}", e);
                            // Skip this document but continue
                            bytes_consumed += if i > 0 {
                                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".len() + part.len()
                            } else {
                                part.len()
                            };
                        }
                    }
                }
            }

            if parsed_any_plist {
                // Remove parsed content from buffer
                if bytes_consumed < buffer.len() {
                    *buffer = buffer[bytes_consumed..].to_vec();
                } else {
                    buffer.clear();
                }
                return Some(events);
            }
        }

        // Fallback: Try to parse as JSON lines (fallback format)
        let buffer_str = String::from_utf8_lossy(buffer);
        let lines: Vec<&str> = buffer_str.lines().collect();
        let mut parsed_lines = 0;
        let mut found_any = false;

        // Process all complete lines (all but potentially the last one)
        for (i, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                parsed_lines = i + 1;
                continue;
            }

            match MetricsEvent::from_json(line) {
                Ok(event) => {
                    events.push(event);
                    found_any = true;
                    parsed_lines = i + 1;
                }
                Err(e) => {
                    debug!("Failed to parse metrics line '{}': {}", line, e);

                    // If this is the last line and the buffer doesn't end with newline,
                    // it might be incomplete - don't count it as parsed
                    if i == lines.len() - 1 && !buffer_str.ends_with('\n') {
                        // This line might be incomplete, keep it in buffer
                        break;
                    } else {
                        // This is a complete but malformed line, skip it
                        parsed_lines = i + 1;
                    }
                }
            }
        }

        if found_any || parsed_lines > 0 {
            // Remove successfully parsed lines from buffer
            let remaining_lines: Vec<&str> = lines.into_iter().skip(parsed_lines).collect();
            let remaining_content = remaining_lines.join("\n");

            // Only add trailing newline if the original buffer had one and we have remaining content
            let new_buffer_content = if !remaining_content.is_empty()
                && buffer_str.ends_with('\n')
                && parsed_lines > 0
            {
                remaining_content + "\n"
            } else {
                remaining_content
            };

            *buffer = new_buffer_content.into_bytes();

            if found_any {
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

impl Drop for MetricsCollector {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::MemoryPressure;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn test_metrics_collector_creation() {
        let (tx, _rx) = mpsc::channel();
        let collector = MetricsCollector::new(Duration::from_secs(5), tx);
        assert!(!collector.is_running());
        assert_eq!(collector.base_sample_interval, Duration::from_secs(5));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_metrics_collector_start_stop() {
        let (tx, _rx) = mpsc::channel();
        let mut collector = MetricsCollector::new(Duration::from_secs(5), tx);

        // Start collector
        let start_result = collector.start();
        // May succeed or fail depending on system permissions and powermetrics availability
        match start_result {
            Ok(_) => {
                assert!(collector.is_running());
                // Stop collector
                assert!(collector.stop().is_ok());
                assert!(!collector.is_running());
            }
            Err(_) => {
                // If start failed, should not be running
                assert!(!collector.is_running());
            }
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_metrics_collector_double_start() {
        let (tx, _rx) = mpsc::channel();
        let mut collector = MetricsCollector::new(Duration::from_secs(5), tx);

        // Start collector twice
        let first_start = collector.start();
        if first_start.is_ok() {
            assert!(collector.start().is_ok()); // Should not error
            assert!(collector.is_running());
            assert!(collector.stop().is_ok());
        }
    }

    #[test]
    fn test_powermetrics_availability_check() {
        // This test checks the availability testing logic
        // It may pass or fail depending on system configuration
        let result = MetricsCollector::test_powermetrics_availability();

        // The test should either succeed or fail gracefully
        match result {
            Ok(_) => {
                // powermetrics is available
            }
            Err(CollectorError::SubprocessSpawn(_)) => {
                // powermetrics is not available, which is acceptable
            }
            Err(e) => {
                panic!("Unexpected error type: {:?}", e);
            }
        }
    }

    #[test]
    fn test_fallback_availability_check() {
        // This test checks if fallback monitoring is available
        let available = MetricsCollector::test_fallback_availability();

        // On macOS, at least one of vm_stat or top should be available
        #[cfg(target_os = "macos")]
        assert!(
            available,
            "At least one fallback monitoring tool should be available on macOS"
        );

        // On other platforms, we don't make assumptions
        #[cfg(not(target_os = "macos"))]
        let _ = available; // Just ensure it doesn't panic
    }

    #[test]
    fn test_parse_buffer_json_format() {
        let json_data = r#"{"timestamp": "2024-12-09T18:30:45.123456Z", "cpu_power_mw": 1234.5, "gpu_power_mw": 567.8, "memory_pressure": "Warning"}
{"timestamp": "2024-12-09T18:30:50.123456Z", "cpu_power_mw": 2000.0, "gpu_power_mw": null, "memory_pressure": "Normal"}"#;

        let mut buffer = json_data.as_bytes().to_vec();
        let events = MetricsCollector::try_parse_buffer(&mut buffer);

        assert!(events.is_some());
        let events = events.unwrap();
        assert_eq!(events.len(), 2);

        assert_eq!(events[0].cpu_power_mw, 1234.5);
        assert_eq!(events[0].gpu_power_mw, Some(567.8));
        assert_eq!(events[0].memory_pressure, MemoryPressure::Warning);

        assert_eq!(events[1].cpu_power_mw, 2000.0);
        assert_eq!(events[1].gpu_power_mw, None);
        assert_eq!(events[1].memory_pressure, MemoryPressure::Normal);
    }

    #[test]
    fn test_parse_buffer_malformed_json() {
        let malformed_data = "invalid json data\n{incomplete json";
        let mut buffer = malformed_data.as_bytes().to_vec();

        // Should handle malformed data gracefully
        let events = MetricsCollector::try_parse_buffer(&mut buffer);

        // Should return None or empty events, not panic
        match events {
            None => {}
            Some(events) => assert!(events.is_empty()),
        }
    }

    #[test]
    fn test_parse_buffer_empty_lines() {
        let data_with_empty_lines = "\n\n\n";
        let mut buffer = data_with_empty_lines.as_bytes().to_vec();

        // Should handle empty lines gracefully
        let events = MetricsCollector::try_parse_buffer(&mut buffer);

        // Should return None or empty events
        match events {
            None => {}
            Some(events) => assert!(events.is_empty()),
        }
    }

    #[test]
    fn test_parse_buffer_split_json_lines() {
        // Test that JSON lines split across read chunks are handled correctly

        // First chunk: complete line + partial line
        let chunk1 = r#"{"timestamp": "2024-12-09T18:30:45.123456Z", "cpu_power_mw": 1234.5, "gpu_power_mw": 567.8, "memory_pressure": "Warning"}
{"timestamp": "2024-12-09T18:30:50.123456Z", "cpu_power_mw""#;

        let mut buffer = chunk1.as_bytes().to_vec();
        let events1 = MetricsCollector::try_parse_buffer(&mut buffer);

        // Should parse the first complete line
        assert!(events1.is_some());
        let events1 = events1.unwrap();
        assert_eq!(events1.len(), 1);
        assert_eq!(events1[0].cpu_power_mw, 1234.5);

        // Buffer should contain the incomplete line
        let remaining = String::from_utf8_lossy(&buffer);
        assert!(
            remaining.contains(r#"{"timestamp": "2024-12-09T18:30:50.123456Z", "cpu_power_mw""#)
        );

        // Second chunk: complete the partial line + new complete line
        let chunk2 = r#": 2000.0, "gpu_power_mw": null, "memory_pressure": "Normal"}
{"timestamp": "2024-12-09T18:30:55.123456Z", "cpu_power_mw": 1500.0, "gpu_power_mw": 800.0, "memory_pressure": "Critical"}
"#;

        // Append the second chunk to the buffer
        buffer.extend_from_slice(chunk2.as_bytes());
        let events2 = MetricsCollector::try_parse_buffer(&mut buffer);

        // Should parse both the completed line and the new complete line
        assert!(events2.is_some());
        let events2 = events2.unwrap();
        assert_eq!(events2.len(), 2);

        // First event (completed from split)
        assert_eq!(events2[0].cpu_power_mw, 2000.0);
        assert_eq!(events2[0].gpu_power_mw, None);
        assert_eq!(events2[0].memory_pressure, MemoryPressure::Normal);

        // Second event (complete from chunk2)
        assert_eq!(events2[1].cpu_power_mw, 1500.0);
        assert_eq!(events2[1].gpu_power_mw, Some(800.0));
        assert_eq!(events2[1].memory_pressure, MemoryPressure::Critical);

        // Buffer should be empty now (all lines were complete)
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_parse_buffer_incomplete_line_preserved() {
        // Test that incomplete lines at the end are preserved without corruption

        let incomplete_data = r#"{"timestamp": "2024-12-09T18:30:45.123456Z", "cpu_power_mw": 1234.5, "gpu_power_mw": 567.8, "memory_pressure": "Warning"}
{"timestamp": "2024-12-09T18:30:50.123456Z", "cpu_power_mw": 2000.0, "gpu_power_mw""#;

        let mut buffer = incomplete_data.as_bytes().to_vec();
        let original_incomplete = r#"{"timestamp": "2024-12-09T18:30:50.123456Z", "cpu_power_mw": 2000.0, "gpu_power_mw""#;

        let events = MetricsCollector::try_parse_buffer(&mut buffer);

        // Should parse the complete line
        assert!(events.is_some());
        let events = events.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].cpu_power_mw, 1234.5);

        // Buffer should contain the incomplete line exactly as it was (no added newline)
        let remaining = String::from_utf8_lossy(&buffer);
        assert_eq!(remaining.trim(), original_incomplete);

        // The incomplete line should not have any extra newlines that would corrupt it
        assert!(!remaining.contains("\n\n"));
        assert_eq!(remaining, original_incomplete); // Exact match, no trailing newline
    }

    #[test]
    fn test_collector_state_consistency() {
        let (tx, _rx) = mpsc::channel();

        // Test with different intervals to verify consistent behavior
        let test_intervals = vec![
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_secs(10),
        ];

        for interval in test_intervals {
            let mut collector = MetricsCollector::new(interval, tx.clone());

            // Initial state should be not running
            assert!(
                !collector.is_running(),
                "Collector should start in not-running state for interval: {:?}",
                interval
            );

            // Test start/stop cycle
            let start_result = collector.start();

            match start_result {
                Ok(_) => {
                    // If start succeeded, should be running
                    assert!(
                        collector.is_running(),
                        "Collector should be running after successful start for interval: {:?}",
                        interval
                    );

                    // Brief pause
                    std::thread::sleep(Duration::from_millis(10));

                    // Stop should work
                    let stop_result = collector.stop();
                    assert!(
                        stop_result.is_ok(),
                        "Stop should succeed for interval: {:?}",
                        interval
                    );
                    assert!(
                        !collector.is_running(),
                        "Collector should not be running after stop for interval: {:?}",
                        interval
                    );
                }
                Err(_) => {
                    // If start failed, should not be running
                    assert!(
                        !collector.is_running(),
                        "Collector should not be running after failed start for interval: {:?}",
                        interval
                    );
                }
            }

            // Final state should be not running
            assert!(
                !collector.is_running(),
                "Collector should end in not-running state for interval: {:?}",
                interval
            );
        }
    }
}

// Property-based tests
#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::events::MemoryPressure;
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Generate valid metrics data for testing
    #[derive(Debug, Clone)]
    struct ValidMetricsData {
        cpu_power_mw: f64,
        gpu_power_mw: Option<f64>,
        memory_pressure: MemoryPressure,
    }

    impl Arbitrary for ValidMetricsData {
        fn arbitrary(g: &mut Gen) -> Self {
            // Generate valid CPU power (0-10000 mW is reasonable for a laptop)
            let cpu_power_mw = (u16::arbitrary(g) % 10001) as f64;

            // Generate optional GPU power (0-50000 mW is reasonable)
            let gpu_power_mw = if bool::arbitrary(g) {
                Some((u16::arbitrary(g) % 50001) as f64)
            } else {
                None
            };

            ValidMetricsData {
                cpu_power_mw,
                gpu_power_mw,
                memory_pressure: MemoryPressure::arbitrary(g),
            }
        }
    }

    impl ValidMetricsData {
        /// Convert to JSON string for testing buffer parsing
        fn to_json(&self) -> String {
            let memory_pressure_str = match self.memory_pressure {
                MemoryPressure::Normal => "Normal",
                MemoryPressure::Warning => "Warning",
                MemoryPressure::Critical => "Critical",
            };

            format!(
                r#"{{"timestamp": "2024-12-09T18:30:45.123456Z", "cpu_power_mw": {}, "gpu_power_mw": {}, "memory_pressure": "{}"}}"#,
                self.cpu_power_mw,
                match self.gpu_power_mw {
                    Some(val) => val.to_string(),
                    None => "null".to_string(),
                },
                memory_pressure_str
            )
        }
    }

    // Feature: macos-system-observer, Property 5: Memory pressure threshold triggers analysis
    // Validates: Requirements 2.5
    #[quickcheck]
    fn prop_memory_pressure_threshold_triggers_analysis(
        metrics_data: Vec<ValidMetricsData>,
        threshold: MemoryPressure,
    ) -> bool {
        // Skip empty input lists or lists that are too large
        if metrics_data.is_empty() || metrics_data.len() > 50 {
            return true;
        }

        // Count metrics that exceed the threshold
        let exceeding_threshold_count = metrics_data
            .iter()
            .filter(|data| data.memory_pressure >= threshold)
            .count();

        // Create JSON data for all metrics
        let json_lines: Vec<String> = metrics_data.iter().map(|data| data.to_json()).collect();
        let json_data = json_lines.join("\n");

        // Parse the buffer
        let mut buffer = json_data.as_bytes().to_vec();
        let parsed_events = MetricsCollector::try_parse_buffer(&mut buffer);

        match parsed_events {
            Some(events) => {
                // Count parsed events that exceed the threshold
                let parsed_exceeding_count = events
                    .iter()
                    .filter(|event| event.memory_pressure >= threshold)
                    .count();

                // The key property: all events exceeding the threshold should be captured
                parsed_exceeding_count == exceeding_threshold_count &&
                // And the total number of parsed events should match the input
                events.len() == metrics_data.len()
            }
            None => {
                // If parsing failed, there should be no events exceeding threshold in the input
                exceeding_threshold_count == 0
            }
        }
    }

    /// Generate malformed metrics data for testing error handling
    #[derive(Debug, Clone)]
    struct MalformedMetricsData(String);

    impl Arbitrary for MalformedMetricsData {
        fn arbitrary(g: &mut Gen) -> Self {
            let malformed_variants: Vec<String> = vec![
                // Incomplete JSON objects
                r#"{"timestamp": "2024-12-09T18:30:45.123456Z""#.to_string(),
                r#"{"cpu_power_mw": 1234.5, "gpu_power_mw":"#.to_string(),
                r#"{"timestamp": "2024-12-09T18:30:45.123456Z", "cpu_power_mw": 1234.5"#.to_string(),

                // Invalid JSON syntax
                "not json at all".to_string(),
                "{ invalid: json }".to_string(),
                "{ \"key\": value }".to_string(),
                "{ \"key\": \"value\", }".to_string(),

                // Missing required fields
                r#"{"timestamp": "2024-12-09T18:30:45.123456Z"}"#.to_string(),
                r#"{"cpu_power_mw": 1234.5}"#.to_string(),
                r#"{"memory_pressure": "Normal"}"#.to_string(),

                // Invalid field types
                r#"{"timestamp": 12345, "cpu_power_mw": 1234.5, "gpu_power_mw": null, "memory_pressure": "Normal"}"#.to_string(),
                r#"{"timestamp": "2024-12-09T18:30:45.123456Z", "cpu_power_mw": "not_a_number", "gpu_power_mw": null, "memory_pressure": "Normal"}"#.to_string(),
                r#"{"timestamp": "2024-12-09T18:30:45.123456Z", "cpu_power_mw": 1234.5, "gpu_power_mw": "invalid", "memory_pressure": "Normal"}"#.to_string(),

                // Invalid memory pressure values
                r#"{"timestamp": "2024-12-09T18:30:45.123456Z", "cpu_power_mw": 1234.5, "gpu_power_mw": null, "memory_pressure": "InvalidPressure"}"#.to_string(),

                // Empty strings and null values
                "".to_string(),
                "null".to_string(),
                "{}".to_string(),
                "[]".to_string(),

                // Very long strings that might cause issues
                format!(r#"{{"timestamp": "{}", "cpu_power_mw": 1234.5, "gpu_power_mw": null, "memory_pressure": "Normal"}}"#, "x".repeat(1000)),

                // Binary data (invalid UTF-8 represented as string)
                String::from_utf8_lossy(&[0xFF, 0xFE, 0xFD]).to_string(),
            ];

            // Choose a random malformed variant or generate a random string
            if bool::arbitrary(g) && !malformed_variants.is_empty() {
                let idx = usize::arbitrary(g) % malformed_variants.len();
                MalformedMetricsData(malformed_variants[idx].clone())
            } else {
                // Generate a random string that's likely to be malformed JSON
                let random_string = String::arbitrary(g);
                MalformedMetricsData(random_string)
            }
        }
    }

    #[quickcheck]
    fn prop_malformed_metrics_dont_halt_processing(malformed: MalformedMetricsData) -> bool {
        // Test that MetricsEvent::from_json handles malformed input gracefully
        let result = MetricsEvent::from_json(&malformed.0);

        // The key property is that parsing should either succeed or fail gracefully
        // It should never panic or cause undefined behavior
        match result {
            Ok(_) => {
                // If it succeeds, that's fine (maybe it was valid JSON after all)
                true
            }
            Err(_) => {
                // If it fails, that's expected for malformed input
                // The important thing is that it returned an error rather than panicking
                true
            }
        }
    }

    #[quickcheck]
    fn prop_buffer_parsing_handles_mixed_content(
        valid_metrics: Vec<ValidMetricsData>,
        malformed_metrics: Vec<MalformedMetricsData>,
    ) -> bool {
        // Skip overly large inputs to avoid long test times
        if valid_metrics.len() > 20 || malformed_metrics.len() > 20 {
            return true;
        }

        // Create mixed content with valid and malformed entries
        let mut all_lines = Vec::new();

        // Add valid entries
        for valid in &valid_metrics {
            all_lines.push(valid.to_json());
        }

        // Add malformed entries (but filter out ones that might actually be valid)
        for malformed in &malformed_metrics {
            if MetricsEvent::from_json(&malformed.0).is_err() {
                all_lines.push(malformed.0.clone());
            }
        }

        // Shuffle the lines to mix valid and invalid entries
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        all_lines.hash(&mut hasher);
        let seed = hasher.finish() as usize;

        // Simple deterministic shuffle based on content hash
        for i in 0..all_lines.len() {
            let j = (seed + i) % all_lines.len();
            all_lines.swap(i, j);
        }

        let mixed_content = all_lines.join("\n");
        let mut buffer = mixed_content.as_bytes().to_vec();

        // Parse the buffer - should not panic
        let parsed_events = MetricsCollector::try_parse_buffer(&mut buffer);

        match parsed_events {
            Some(events) => {
                // Should have parsed exactly the number of valid entries
                events.len() == valid_metrics.len()
            }
            None => {
                // If no events were parsed, there should be no valid entries
                valid_metrics.is_empty()
            }
        }
    }

    #[quickcheck]
    fn prop_collector_state_management_consistency(intervals: Vec<u64>) -> bool {
        // Limit the number of intervals to test to avoid excessive test time
        let test_intervals: Vec<Duration> = intervals
            .into_iter()
            .take(3) // Limit to 3 intervals
            .filter(|&i| i > 0 && i <= 60) // Reasonable range: 1-60 seconds
            .map(Duration::from_secs)
            .collect();

        if test_intervals.is_empty() {
            return true; // Skip if no valid intervals
        }

        let (tx, _rx) = mpsc::channel();

        for interval in test_intervals {
            let mut collector = MetricsCollector::new(interval, tx.clone());

            // Initial state should be not running
            if collector.is_running() {
                return false;
            }

            // Test start
            let start_result = collector.start();
            match start_result {
                Ok(_) => {
                    // If start succeeded, should be running
                    if !collector.is_running() {
                        return false;
                    }

                    // Brief pause to let thread initialize
                    std::thread::sleep(Duration::from_millis(1));

                    // Stop should work
                    if collector.stop().is_err() {
                        return false;
                    }

                    // Should be stopped after stop
                    if collector.is_running() {
                        return false;
                    }
                }
                Err(_) => {
                    // If start failed, should not be running
                    if collector.is_running() {
                        return false;
                    }
                }
            }

            // Final state should be not running
            if collector.is_running() {
                return false;
            }
        }

        true
    }
}
