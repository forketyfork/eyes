use crate::ai::AIInsight;
use crate::alerts::{AlertStatus, AlertStore, RateLimiter};
use crate::error::AlertError;
use crate::events::Severity;
use crate::monitoring::SelfMonitoringCollector;
use crate::triggers::TriggerContext;
use log::{error, info, warn};
use std::collections::VecDeque;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Manages delivery of system alerts via macOS notifications
///
/// The AlertManager coordinates with the RateLimiter to prevent notification spam
/// and uses osascript to deliver native macOS notifications. It handles failures
/// gracefully to ensure that notification issues don't disrupt system monitoring.
///
/// The AlertManager automatically processes queued alerts whenever send_alert is called,
/// and provides autonomous processing via the tick() method that should be called
/// periodically to ensure queued alerts are delivered when rate limiting capacity
/// becomes available, even if no new alerts arrive.
#[derive(Debug)]
struct QueuedAlert {
    id: Option<i64>,
    insight: AIInsight,
}

#[derive(Debug)]
pub struct AlertManager {
    /// Rate limiter to prevent notification spam
    rate_limiter: RateLimiter,
    /// Queue for alerts that are rate limited
    alert_queue: VecDeque<QueuedAlert>,
    /// Maximum size of the alert queue
    max_queue_size: usize,
    /// Lowest severity delivered as a notification
    minimum_severity: Severity,
    /// Whether native macOS desktop notifications may be delivered
    desktop_notifications_enabled: bool,
    /// Whether to use mock notifications for testing
    use_mock_notifications: bool,
    /// Self-monitoring collector for tracking notification success/failure
    monitoring: Option<Arc<SelfMonitoringCollector>>,
    /// SQLite persistence for alert and assessment history
    store: Option<AlertStore>,
    #[cfg(test)]
    mock_notification_failures: VecDeque<bool>,
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new(3) // Default: 3 notifications per minute
    }
}

impl AlertManager {
    /// Create a new alert manager with the specified rate limit and default queue size
    ///
    /// # Arguments
    ///
    /// * `max_per_minute` - Maximum number of notifications allowed per minute
    pub fn new(max_per_minute: usize) -> Self {
        Self::with_queue_size(max_per_minute, 100)
    }

    /// Create a new alert manager with configurable rate limit and queue size
    ///
    /// # Arguments
    ///
    /// * `max_per_minute` - Maximum number of notifications allowed per minute
    /// * `max_queue_size` - Maximum number of alerts to queue when rate limited
    pub fn with_queue_size(max_per_minute: usize, max_queue_size: usize) -> Self {
        Self::with_minimum_severity(max_per_minute, max_queue_size, Severity::Critical)
    }

    /// Create an alert manager with configurable queue size and severity threshold.
    pub fn with_minimum_severity(
        max_per_minute: usize,
        max_queue_size: usize,
        minimum_severity: Severity,
    ) -> Self {
        Self {
            rate_limiter: RateLimiter::new(max_per_minute),
            alert_queue: VecDeque::new(),
            max_queue_size,
            minimum_severity,
            desktop_notifications_enabled: false,
            use_mock_notifications: false,
            monitoring: None,
            store: None,
            #[cfg(test)]
            mock_notification_failures: VecDeque::new(),
        }
    }

    /// Create a new alert manager for testing with mock notifications
    ///
    /// # Arguments
    ///
    /// * `max_per_minute` - Maximum number of notifications allowed per minute
    pub fn new_for_testing(max_per_minute: usize) -> Self {
        Self::new_for_testing_with_queue_size(max_per_minute, 100)
    }

    /// Create a new alert manager for testing with configurable queue size
    ///
    /// # Arguments
    ///
    /// * `max_per_minute` - Maximum number of notifications allowed per minute
    /// * `max_queue_size` - Maximum number of alerts to queue when rate limited
    pub fn new_for_testing_with_queue_size(max_per_minute: usize, max_queue_size: usize) -> Self {
        Self {
            rate_limiter: RateLimiter::new(max_per_minute),
            alert_queue: VecDeque::new(),
            max_queue_size,
            minimum_severity: Severity::Critical,
            desktop_notifications_enabled: true,
            use_mock_notifications: true,
            monitoring: None,
            store: None,
            #[cfg(test)]
            mock_notification_failures: VecDeque::new(),
        }
    }

    /// Create an alert manager that persists alert history to SQLite.
    pub fn with_database(
        max_per_minute: usize,
        max_queue_size: usize,
        minimum_severity: Severity,
        database_path: &Path,
    ) -> Result<Self, AlertError> {
        let mut manager =
            Self::with_minimum_severity(max_per_minute, max_queue_size, minimum_severity);
        let store = AlertStore::open(database_path)?;
        let interrupted = store
            .fail_pending_candidates("Eyes restarted before the pending analysis could complete")?;
        if interrupted > 0 {
            warn!(
                "Marked {} interrupted alert analyses as failed",
                interrupted
            );
        }
        manager.store = Some(store);
        Ok(manager)
    }

    /// Set the self-monitoring collector for tracking notification success/failure
    pub fn set_monitoring(&mut self, monitoring: Arc<SelfMonitoringCollector>) {
        self.monitoring = Some(monitoring);
    }

    pub fn set_desktop_notifications_enabled(&mut self, enabled: bool) {
        self.desktop_notifications_enabled = enabled;
    }

    /// Send an alert based on an AI insight
    ///
    /// This method formats the insight into a macOS notification and delivers it
    /// via osascript, respecting rate limits and handling failures gracefully.
    /// Alerts are queued when rate limited and automatically processed whenever
    /// send_alert is called and rate capacity becomes available.
    ///
    /// # Arguments
    ///
    /// * `insight` - The AI-generated insight to alert about
    ///
    /// # Returns
    ///
    /// `Ok(())` if the notification was sent successfully or queued,
    /// `Err(AlertError)` if there was a critical failure
    ///
    /// # Errors
    ///
    /// Returns `AlertError::NotificationFailed` if osascript execution fails.
    pub fn send_alert(&mut self, insight: &AIInsight) -> Result<(), AlertError> {
        self.send_alert_with_candidate(None, insight)
    }

    pub fn send_alert_for_candidate(
        &mut self,
        candidate_id: Option<i64>,
        insight: &AIInsight,
    ) -> Result<(), AlertError> {
        self.send_alert_with_candidate(candidate_id, insight)
    }

    pub fn record_analysis_candidate(
        &mut self,
        context: &TriggerContext,
    ) -> Result<Option<i64>, AlertError> {
        self.store
            .as_mut()
            .map(|store| store.record_candidate(context))
            .transpose()
    }

    pub fn mark_analysis_failed(&self, candidate_id: Option<i64>, failure_message: &str) {
        let (Some(store), Some(candidate_id)) = (&self.store, candidate_id) else {
            return;
        };
        if let Err(error) = store.mark_candidate_failed(candidate_id, failure_message) {
            error!(
                "Failed to mark alert candidate {} as unanalyzed: {}",
                candidate_id, error
            );
        }
    }

    fn send_alert_with_candidate(
        &mut self,
        candidate_id: Option<i64>,
        insight: &AIInsight,
    ) -> Result<(), AlertError> {
        use log::debug;

        debug!(
            "Processing alert request: severity={:?}, summary='{}'",
            insight.severity, insight.summary
        );

        if !self.desktop_notifications_enabled {
            self.persist_alert(candidate_id, insight, AlertStatus::Suppressed);
            info!(
                "Desktop notifications disabled; assessment retained in dashboard: {}",
                insight.summary
            );
            return Ok(());
        }

        // First, automatically process any queued alerts
        let processed_count = self.alert_queue.len();
        let queued_result = self.process_queued_alerts();
        let remaining_count = self.alert_queue.len();

        match queued_result {
            Ok(()) if processed_count > remaining_count => {
                debug!(
                    "Processed {} queued alerts during send_alert",
                    processed_count - remaining_count
                );
            }
            Err(error) => {
                error!(
                    "Failed to process queued alerts before '{}': {}",
                    insight.summary, error
                );
            }
            Ok(()) => {}
        }

        if insight.severity < self.minimum_severity {
            self.persist_alert(candidate_id, insight, AlertStatus::Suppressed);
            info!(
                "Skipping notification below configured severity ({}): {}",
                format!("{:?}", insight.severity).to_lowercase(),
                insight.summary
            );
            return Ok(());
        }

        debug!(
            "Processing alert: rate_limit_available={}",
            self.rate_limiter.can_send()
        );

        // Try to send the alert immediately
        if self.rate_limiter.can_send() {
            debug!("Sending notification immediately");
            let alert_id = self.persist_alert(candidate_id, insight, AlertStatus::Pending);
            self.send_notification_now(alert_id, insight)
        } else {
            // Queue the alert for later processing
            debug!("Rate limit exceeded, queueing alert");
            let alert_id = self.persist_alert(candidate_id, insight, AlertStatus::Queued);
            self.queue_alert(alert_id, insight.clone());
            info!(
                "Queued notification due to rate limit: {} (queue size: {})",
                insight.summary,
                self.alert_queue.len()
            );
            Ok(())
        }
    }

    /// Process queued alerts if rate limiting allows
    fn process_queued_alerts(&mut self) -> Result<(), AlertError> {
        use log::debug;

        if !self.desktop_notifications_enabled {
            return Ok(());
        }

        let _initial_queue_size = self.alert_queue.len();
        let mut processed_count = 0;

        while !self.alert_queue.is_empty() && self.rate_limiter.can_send() {
            if let Some(queued_alert) = self.alert_queue.pop_front() {
                if queued_alert.insight.severity >= self.minimum_severity {
                    debug!(
                        "Processing queued alert: '{}'",
                        queued_alert.insight.summary
                    );
                    self.send_notification_now(queued_alert.id, &queued_alert.insight)?;
                    processed_count += 1;
                } else {
                    debug!(
                        "Skipping queued non-critical alert: '{}'",
                        queued_alert.insight.summary
                    );
                    self.update_alert_status(queued_alert.id, AlertStatus::Suppressed, None);
                }
            }
        }

        if processed_count > 0 {
            debug!(
                "Processed {} queued alerts, {} remaining in queue",
                processed_count,
                self.alert_queue.len()
            );
        }

        Ok(())
    }

    /// Queue an alert for later processing
    fn queue_alert(&mut self, alert_id: Option<i64>, insight: AIInsight) {
        use log::debug;

        if self.alert_queue.len() >= self.max_queue_size {
            // Drop the oldest alert to make room
            if let Some(dropped) = self.alert_queue.pop_front() {
                warn!(
                    "Alert queue full (max: {}), dropping oldest alert: '{}'",
                    self.max_queue_size, dropped.insight.summary
                );
                self.update_alert_status(
                    dropped.id,
                    AlertStatus::Dropped,
                    Some("alert queue capacity exceeded"),
                );
            }
        }

        debug!(
            "Queueing alert: '{}' (queue size: {}/{})",
            insight.summary,
            self.alert_queue.len() + 1,
            self.max_queue_size
        );

        self.alert_queue.push_back(QueuedAlert {
            id: alert_id,
            insight,
        });
    }

    /// Send a notification immediately (assumes rate limit check has passed)
    fn send_notification_now(
        &mut self,
        alert_id: Option<i64>,
        insight: &AIInsight,
    ) -> Result<(), AlertError> {
        use log::debug;

        // Format the notification with truncation
        let title = Self::truncate_text(&format!("System Alert: {}", insight.summary), 256);
        let body = Self::truncate_text(&Self::format_notification_body(insight), 1024);

        debug!(
            "Formatted notification: title_len={}, body_len={}, recommendations_count={}",
            title.len(),
            body.len(),
            insight.recommendations.len()
        );

        // Send the notification
        match self.send_macos_notification(&title, &body) {
            Ok(()) => {
                self.rate_limiter.record_notification();
                let details = Self::format_log_entry(insight);
                match insight.severity {
                    Severity::Critical => error!("{}", details),
                    Severity::Warning => warn!("{}", details),
                    Severity::Info => info!("{}", details),
                }
                debug!(
                    "Current notification count: {}",
                    self.rate_limiter.current_count()
                );

                // Record successful delivery in monitoring
                if let Some(ref monitoring) = self.monitoring {
                    monitoring.record_notification_result(true);
                }

                self.update_alert_status(alert_id, AlertStatus::Delivered, None);

                Ok(())
            }
            Err(e) => {
                error!("Failed to send notification '{}': {}", insight.summary, e);

                self.update_alert_status(
                    alert_id,
                    AlertStatus::DeliveryFailed,
                    Some(&e.to_string()),
                );

                // Record failed delivery in monitoring
                if let Some(ref monitoring) = self.monitoring {
                    monitoring.record_notification_result(false);
                }

                Err(e)
            }
        }
    }

    fn persist_alert(
        &mut self,
        candidate_id: Option<i64>,
        insight: &AIInsight,
        status: AlertStatus,
    ) -> Option<i64> {
        let store = self.store.as_mut()?;
        let title = Self::truncate_text(&format!("System Alert: {}", insight.summary), 256);
        let body = Self::truncate_text(&Self::format_notification_body(insight), 1024);
        match store.record_alert_for_candidate(candidate_id, insight, &title, &body, status) {
            Ok(alert_id) => Some(alert_id),
            Err(error) => {
                error!(
                    "Failed to persist alert history for '{}': {}",
                    insight.summary, error
                );
                if let Some(candidate_id) = candidate_id {
                    if let Err(mark_error) = store.mark_candidate_failed(
                        candidate_id,
                        "analysis completed but its assessment could not be persisted",
                    ) {
                        error!(
                            "Failed to mark alert candidate {} after persistence failure: {}",
                            candidate_id, mark_error
                        );
                    }
                }
                None
            }
        }
    }

    fn update_alert_status(
        &self,
        alert_id: Option<i64>,
        status: AlertStatus,
        failure_message: Option<&str>,
    ) {
        let (Some(store), Some(alert_id)) = (&self.store, alert_id) else {
            return;
        };
        if let Err(error) = store.update_status(alert_id, status, failure_message) {
            error!(
                "Failed to update alert history for alert {} to {:?}: {}",
                alert_id, status, error
            );
        }
    }

    /// Truncate text to a maximum length for notification limits
    ///
    /// This method properly handles UTF-8 character boundaries to avoid panics
    /// when truncating strings containing non-ASCII characters.
    fn truncate_text(text: &str, max_length: usize) -> String {
        if text.len() <= max_length {
            text.to_string()
        } else {
            // Find a safe UTF-8 boundary for truncation
            let mut truncate_at = max_length.saturating_sub(3);

            // Walk backwards to find a valid UTF-8 character boundary
            while truncate_at > 0 && !text.is_char_boundary(truncate_at) {
                truncate_at -= 1;
            }

            // If we couldn't find a boundary, just take the first few characters
            if truncate_at == 0 {
                // Take the first few characters that fit
                let chars: Vec<char> = text.chars().collect();
                let mut result = String::new();
                for ch in chars.iter().take(max_length.saturating_sub(3)) {
                    if result.len() + ch.len_utf8() <= max_length.saturating_sub(3) {
                        result.push(*ch);
                    } else {
                        break;
                    }
                }
                format!("{}...", result)
            } else {
                format!("{}...", &text[..truncate_at])
            }
        }
    }

    /// Format the notification body from an AI insight
    ///
    /// Creates a concise but informative notification body that includes
    /// the root cause (if available) and up to 3 recommendations.
    ///
    /// # Arguments
    ///
    /// * `insight` - The AI insight to format
    ///
    /// # Returns
    ///
    /// Formatted notification body text
    fn format_notification_body(insight: &AIInsight) -> String {
        let mut body = String::new();

        // Add root cause if available
        if let Some(ref root_cause) = insight.root_cause {
            body.push_str(&format!("Cause: {}\n\n", root_cause));
        }

        body.push_str(&format!(
            "Observation confidence: {}\nDiagnosis confidence: {}\n",
            insight.observation_confidence, insight.diagnosis_confidence
        ));
        for observation in insight.evidence.iter().take(2) {
            body.push_str(&format!("Evidence: {}\n", observation));
        }

        if !insight.limitations.is_empty() {
            body.push_str(&format!("Limitation: {}\n", insight.limitations[0]));
        }

        if !body.is_empty() {
            body.push('\n');
        }

        // Add recommendations (limit to 3 for readability)
        if !insight.recommendations.is_empty() {
            body.push_str("Recommendations:\n");
            for (i, recommendation) in insight.recommendations.iter().take(3).enumerate() {
                body.push_str(&format!("{}. {}\n", i + 1, recommendation));
            }

            // Indicate if there are more recommendations
            if insight.recommendations.len() > 3 {
                body.push_str(&format!(
                    "... and {} more recommendations",
                    insight.recommendations.len() - 3
                ));
            }
        }

        body.trim().to_string()
    }

    fn format_log_entry(insight: &AIInsight) -> String {
        let root_cause = insight.root_cause.as_deref().unwrap_or("Not provided");
        let recommendations = if insight.recommendations.is_empty() {
            "- None".to_string()
        } else {
            insight
                .recommendations
                .iter()
                .map(|recommendation| format!("- {}", recommendation))
                .collect::<Vec<_>>()
                .join("\n")
        };

        format!(
            "Delivered system alert ({:?})\nSummary: {}\nRoot cause: {}\nObservation confidence: {}\nDiagnosis confidence: {}\nEvidence: {}\nLimitations: {}\nRecommendations:\n{}",
            insight.severity,
            insight.summary,
            root_cause,
            insight.observation_confidence,
            insight.diagnosis_confidence,
            if insight.evidence.is_empty() {
                "None".to_string()
            } else {
                insight.evidence.join("; ")
            },
            if insight.limitations.is_empty() {
                "None".to_string()
            } else {
                insight.limitations.join("; ")
            },
            recommendations
        )
    }

    /// Send a macOS notification using osascript or mock for testing
    ///
    /// Uses AppleScript via osascript to trigger a native macOS notification.
    /// This ensures the notification appears in the system notification center
    /// and respects user notification preferences. In testing mode, notifications
    /// are mocked to avoid spamming the user.
    ///
    /// # Arguments
    ///
    /// * `title` - Notification title
    /// * `body` - Notification body text
    ///
    /// # Returns
    ///
    /// `Ok(())` if the notification was sent successfully,
    /// `Err(AlertError)` if osascript execution failed
    fn send_macos_notification(&mut self, title: &str, body: &str) -> Result<(), AlertError> {
        if self.use_mock_notifications {
            #[cfg(test)]
            if self.mock_notification_failures.pop_front().unwrap_or(false) {
                return Err(AlertError::NotificationFailed(
                    "mock notification failure".to_string(),
                ));
            }
            // Mock notification for testing - just log it
            info!("MOCK NOTIFICATION - Title: {}, Body: {}", title, body);
            return Ok(());
        }

        let output = Self::notification_command(title, body)
            .output()
            .map_err(|e| {
                AlertError::NotificationFailed(format!("Failed to execute osascript: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AlertError::NotificationFailed(format!(
                "osascript failed with status {}: {}",
                output.status, stderr
            )));
        }

        Ok(())
    }

    fn notification_command(title: &str, body: &str) -> Command {
        let script = r#"on run argv
            display notification (item 2 of argv) with title (item 1 of argv)
        end run"#;
        let mut command = Command::new("osascript");
        command.arg("-e").arg(script).arg("--").arg(title).arg(body);
        command
    }

    /// Get the current notification count in the rate limiting window
    ///
    /// This is primarily used for testing and monitoring.
    ///
    /// # Returns
    ///
    /// Number of notifications sent in the current time window
    pub fn current_notification_count(&mut self) -> usize {
        self.rate_limiter.current_count()
    }

    /// Check if a notification can be sent without exceeding rate limits
    ///
    /// This is primarily used for testing and monitoring.
    ///
    /// # Returns
    ///
    /// `true` if a notification can be sent, `false` if rate limited
    pub fn can_send_notification(&mut self) -> bool {
        self.desktop_notifications_enabled && self.rate_limiter.can_send()
    }

    /// Get the current number of queued alerts
    ///
    /// This is primarily used for testing and monitoring.
    ///
    /// # Returns
    ///
    /// Number of alerts currently in the queue
    pub fn queued_alert_count(&self) -> usize {
        self.alert_queue.len()
    }

    /// Process queued alerts independently of new alert submissions
    ///
    /// This method processes any queued alerts when rate limiting capacity becomes available.
    /// It's automatically called by send_alert, but can also be called manually.
    ///
    /// # Returns
    ///
    /// `Ok(usize)` with the number of alerts processed, or `Err(AlertError)` if processing failed
    pub fn process_queue(&mut self) -> Result<usize, AlertError> {
        let initial_queue_size = self.alert_queue.len();
        self.process_queued_alerts()?;
        let processed_count = initial_queue_size - self.alert_queue.len();

        if processed_count > 0 {
            info!("Processed {} queued alerts", processed_count);
        }

        Ok(processed_count)
    }

    /// Check if there are queued alerts that can be processed
    ///
    /// This method checks if there are any queued alerts and if the rate limiter
    /// would allow sending them. It's designed to be called periodically by the
    /// main application loop to enable autonomous queue processing.
    ///
    /// # Returns
    ///
    /// `true` if there are queued alerts and rate limiting allows processing them
    pub fn has_processable_alerts(&mut self) -> bool {
        self.desktop_notifications_enabled
            && !self.alert_queue.is_empty()
            && self.rate_limiter.can_send()
    }

    /// Autonomously process queued alerts if rate limiting allows
    ///
    /// This method should be called periodically (e.g., every 10-30 seconds) by the
    /// main application loop to ensure queued alerts are delivered when rate limiting
    /// capacity becomes available, even if no new alerts arrive.
    ///
    /// # Returns
    ///
    /// `Ok(usize)` with the number of alerts processed, or `Err(AlertError)` if processing failed
    pub fn tick(&mut self) -> Result<usize, AlertError> {
        if self.has_processable_alerts() {
            self.process_queue()
        } else {
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Severity;
    use chrono::Utc;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn create_test_insight(severity: Severity, summary: &str) -> AIInsight {
        AIInsight {
            timestamp: Utc::now(),
            summary: summary.to_string(),
            root_cause: Some("Test root cause".to_string()),
            recommendations: vec![
                "First recommendation".to_string(),
                "Second recommendation".to_string(),
                "Third recommendation".to_string(),
            ],
            evidence: vec!["Observed test condition".to_string()],
            observation_confidence: "high".to_string(),
            diagnosis_confidence: "medium".to_string(),
            limitations: vec!["Test context only".to_string()],
            severity,
        }
    }

    #[test]
    fn test_alert_manager_creation() {
        let mut manager = AlertManager::new_for_testing(5);
        // Test that the rate limiter is working by checking we can send up to 5 notifications
        for _ in 0..5 {
            assert!(manager.can_send_notification());
            manager.rate_limiter.record_notification();
        }
        // 6th should be blocked
        assert!(!manager.can_send_notification());
    }

    #[test]
    fn test_format_notification_body_with_root_cause() {
        let insight = create_test_insight(Severity::Critical, "Test alert");

        let body = AlertManager::format_notification_body(&insight);

        assert!(body.contains("Cause: Test root cause"));
        assert!(body.contains("Recommendations:"));
        assert!(body.contains("1. First recommendation"));
        assert!(body.contains("2. Second recommendation"));
        assert!(body.contains("3. Third recommendation"));
    }

    #[test]
    fn test_format_notification_body_without_root_cause() {
        let mut insight = create_test_insight(Severity::Critical, "Test alert");
        insight.root_cause = None;

        let body = AlertManager::format_notification_body(&insight);

        assert!(!body.contains("Cause:"));
        assert!(body.contains("Recommendations:"));
        assert!(body.contains("1. First recommendation"));
    }

    #[test]
    fn test_format_notification_body_many_recommendations() {
        let mut insight = create_test_insight(Severity::Critical, "Test alert");
        insight.recommendations = vec![
            "First".to_string(),
            "Second".to_string(),
            "Third".to_string(),
            "Fourth".to_string(),
            "Fifth".to_string(),
        ];

        let body = AlertManager::format_notification_body(&insight);

        assert!(body.contains("1. First"));
        assert!(body.contains("2. Second"));
        assert!(body.contains("3. Third"));
        assert!(!body.contains("4. Fourth"));
        assert!(body.contains("... and 2 more recommendations"));
    }

    #[test]
    fn test_format_notification_body_no_recommendations() {
        let mut insight = create_test_insight(Severity::Critical, "Test alert");
        insight.recommendations = vec![];

        let body = AlertManager::format_notification_body(&insight);

        assert!(body.contains("Cause: Test root cause"));
        assert!(!body.contains("Recommendations:"));
    }

    #[test]
    fn test_format_log_entry_includes_complete_insight() {
        let mut insight = create_test_insight(Severity::Warning, "Disk activity increased");
        insight.recommendations = vec![
            "Inspect active processes".to_string(),
            "Review scheduled jobs".to_string(),
            "Check storage latency".to_string(),
            "Keep monitoring the disk".to_string(),
        ];

        let entry = AlertManager::format_log_entry(&insight);

        assert!(entry.contains("Delivered system alert (Warning)"));
        assert!(entry.contains("Summary: Disk activity increased"));
        assert!(entry.contains("Root cause: Test root cause"));
        assert!(entry.contains("Observation confidence: high"));
        assert!(entry.contains("Diagnosis confidence: medium"));
        assert!(entry.contains("- Keep monitoring the disk"));
    }

    #[test]
    fn test_notification_content_is_passed_as_arguments() {
        let title = r#"Daemon reported \"invalid policy\""#;
        let body = "Cause: path \\System\\Policy\nRecommendation: don't retry 🚨";

        let command = AlertManager::notification_command(title, body);
        let arguments: Vec<_> = command.get_args().collect();

        assert_eq!(command.get_program(), "osascript");
        assert_eq!(arguments[0], "-e");
        assert_eq!(arguments[2], "--");
        assert_eq!(arguments[3], title);
        assert_eq!(arguments[4], body);
        assert!(!arguments[1].to_string_lossy().contains(title));
        assert!(!arguments[1].to_string_lossy().contains(body));
    }

    #[test]
    fn test_send_alert_skips_non_critical() {
        let mut manager = AlertManager::new_for_testing(3);
        let insight = create_test_insight(Severity::Warning, "Warning alert");

        // Should succeed but not actually send notification
        let result = manager.send_alert(&insight);
        assert!(result.is_ok());

        // Rate limiter should not be affected
        assert_eq!(manager.current_notification_count(), 0);
    }

    #[test]
    fn test_rate_limiting_integration() {
        let mut manager = AlertManager::new_for_testing(1); // Very restrictive limit
        let insight = create_test_insight(Severity::Critical, "Critical alert");

        // First alert should be sent successfully
        let result1 = manager.send_alert(&insight);
        assert!(result1.is_ok());
        assert_eq!(manager.current_notification_count(), 1);

        // Second alert should be queued due to rate limit
        let result2 = manager.send_alert(&insight);
        assert!(result2.is_ok());
        assert_eq!(manager.queued_alert_count(), 1);
        assert_eq!(manager.current_notification_count(), 1); // Still only 1 sent
    }

    #[test]
    fn test_alert_queueing() {
        let mut manager = AlertManager::new_for_testing(1);
        let insight = create_test_insight(Severity::Critical, "Critical alert");

        // Fill up the rate limit
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.current_notification_count(), 1);

        // Next alerts should be queued
        manager.send_alert(&insight).unwrap();
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.queued_alert_count(), 2);

        // Process queued alerts (simulate time passing)
        manager.rate_limiter = RateLimiter::new(5); // Reset rate limiter
        let processed = manager.process_queue().unwrap();

        // All queued alerts should be processed
        assert_eq!(processed, 2);
        assert_eq!(manager.queued_alert_count(), 0);
    }

    #[test]
    fn test_configured_warning_alert_is_delivered() {
        let mut manager = AlertManager::with_minimum_severity(3, 100, Severity::Warning);
        manager.use_mock_notifications = true;
        manager.set_desktop_notifications_enabled(true);
        let insight = create_test_insight(Severity::Warning, "Warning alert");

        manager.send_alert(&insight).unwrap();

        assert_eq!(manager.current_notification_count(), 1);
    }

    #[test]
    fn desktop_notifications_are_disabled_by_default() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("alerts.db");
        let mut manager =
            AlertManager::with_database(3, 100, Severity::Warning, &database_path).unwrap();
        manager.use_mock_notifications = true;

        manager
            .send_alert(&create_test_insight(Severity::Critical, "Critical alert"))
            .unwrap();

        assert_eq!(manager.current_notification_count(), 0);
        assert_eq!(manager.queued_alert_count(), 0);
        let status: String = Connection::open(database_path)
            .unwrap()
            .query_row("SELECT status FROM alerts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "suppressed");
    }

    #[test]
    fn test_queued_failure_does_not_mask_current_alert_success() {
        let mut manager = AlertManager::new_for_testing(1);
        let queued = create_test_insight(Severity::Critical, "Queued alert");

        manager.send_alert(&queued).unwrap();
        manager.send_alert(&queued).unwrap();
        manager.rate_limiter = RateLimiter::new(2);
        manager.mock_notification_failures.extend([true, false]);

        let current = create_test_insight(Severity::Critical, "Current alert");
        assert!(manager.send_alert(&current).is_ok());
        assert_eq!(manager.current_notification_count(), 1);
        assert_eq!(manager.queued_alert_count(), 0);
    }

    #[test]
    fn test_queued_failure_does_not_mask_suppressed_alert_success() {
        let mut manager = AlertManager::new_for_testing(1);
        let queued = create_test_insight(Severity::Critical, "Queued alert");

        manager.send_alert(&queued).unwrap();
        manager.send_alert(&queued).unwrap();
        manager.rate_limiter = RateLimiter::new(2);
        manager.mock_notification_failures.push_back(true);

        let suppressed = create_test_insight(Severity::Warning, "Suppressed alert");
        assert!(manager.send_alert(&suppressed).is_ok());
        assert_eq!(manager.current_notification_count(), 0);
        assert_eq!(manager.queued_alert_count(), 0);
    }

    #[test]
    fn test_database_records_suppressed_and_delivered_alerts() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("alerts.db");
        let mut manager =
            AlertManager::with_database(3, 100, Severity::Critical, &database_path).unwrap();
        manager.use_mock_notifications = true;
        manager.set_desktop_notifications_enabled(true);

        manager
            .send_alert(&create_test_insight(Severity::Warning, "Suppressed"))
            .unwrap();
        manager
            .send_alert(&create_test_insight(Severity::Critical, "Delivered"))
            .unwrap();

        let connection = Connection::open(database_path).unwrap();
        let statuses: Vec<String> = connection
            .prepare("SELECT status FROM alerts ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(statuses, vec!["suppressed", "delivered"]);
    }

    #[test]
    fn test_database_tracks_queue_overflow() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("alerts.db");
        let mut manager =
            AlertManager::with_database(1, 1, Severity::Critical, &database_path).unwrap();
        manager.use_mock_notifications = true;
        manager.set_desktop_notifications_enabled(true);
        let insight = create_test_insight(Severity::Critical, "Critical");

        manager.send_alert(&insight).unwrap();
        manager.send_alert(&insight).unwrap();
        manager.send_alert(&insight).unwrap();

        let connection = Connection::open(database_path).unwrap();
        let statuses: Vec<String> = connection
            .prepare("SELECT status FROM alerts ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(statuses, vec!["delivered", "dropped", "queued"]);
    }

    #[test]
    fn test_database_insert_failure_does_not_suppress_notification() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("alerts.db");
        let mut manager =
            AlertManager::with_database(3, 100, Severity::Critical, &database_path).unwrap();
        manager.use_mock_notifications = true;
        manager.set_desktop_notifications_enabled(true);
        manager
            .store
            .as_ref()
            .unwrap()
            .execute_batch_for_testing(
                "CREATE TRIGGER reject_assessment_insert
                 BEFORE INSERT ON assessments
                 BEGIN SELECT RAISE(FAIL, 'injected insert failure'); END;",
            )
            .unwrap();

        let insight = create_test_insight(Severity::Critical, "Critical alert");
        assert!(manager.send_alert(&insight).is_ok());
        assert_eq!(manager.current_notification_count(), 1);

        let connection = Connection::open(database_path).unwrap();
        let alert_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM alerts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(alert_count, 0);
    }

    #[test]
    fn test_database_status_failure_preserves_delivery_accounting() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("alerts.db");
        let mut manager =
            AlertManager::with_database(3, 100, Severity::Critical, &database_path).unwrap();
        manager.use_mock_notifications = true;
        manager.set_desktop_notifications_enabled(true);
        let monitoring = Arc::new(SelfMonitoringCollector::new());
        manager.set_monitoring(monitoring.clone());
        manager
            .store
            .as_ref()
            .unwrap()
            .execute_batch_for_testing(
                "CREATE TRIGGER reject_alert_update
                 BEFORE UPDATE ON alerts
                 BEGIN SELECT RAISE(FAIL, 'injected update failure'); END;",
            )
            .unwrap();

        let insight = create_test_insight(Severity::Critical, "Critical alert");
        assert!(manager.send_alert(&insight).is_ok());
        assert_eq!(manager.current_notification_count(), 1);
        assert_eq!(
            monitoring
                .collect_metrics()
                .successful_notifications_per_minute,
            1
        );

        let connection = Connection::open(database_path).unwrap();
        let status: String = connection
            .query_row("SELECT status FROM alerts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn test_text_truncation() {
        // Test title truncation
        let long_text = "a".repeat(300);
        let truncated = AlertManager::truncate_text(&long_text, 256);
        assert!(truncated.len() <= 256);
        assert!(truncated.ends_with("..."));

        // Test body truncation
        let very_long_text = "b".repeat(2000);
        let truncated_body = AlertManager::truncate_text(&very_long_text, 1024);
        assert!(truncated_body.len() <= 1024);
        assert!(truncated_body.ends_with("..."));

        // Test no truncation needed
        let short_text = "short";
        let not_truncated = AlertManager::truncate_text(short_text, 256);
        assert_eq!(not_truncated, short_text);

        // Test UTF-8 truncation safety
        let unicode_text = "Hello 世界! This is a test with unicode characters 🚀🎉";
        let truncated_unicode = AlertManager::truncate_text(unicode_text, 20);
        assert!(truncated_unicode.len() <= 20);
        assert!(truncated_unicode.ends_with("..."));
        // Should not panic and should be valid UTF-8
        assert!(truncated_unicode.is_ascii() || truncated_unicode.chars().count() > 0);

        // Test edge case with very short limit
        let short_limit = AlertManager::truncate_text("Hello 世界", 5);
        assert!(short_limit.len() <= 5);
        assert!(short_limit.ends_with("..."));
    }

    #[test]
    fn test_utf8_truncation_safety() {
        // Test various Unicode strings that could cause byte boundary issues
        let test_cases = vec![
            "🚀🎉🌟",            // Emojis (4 bytes each)
            "世界你好",          // Chinese characters (3 bytes each)
            "Здравствуй мир",    // Cyrillic (2 bytes each)
            "café résumé naïve", // Accented characters
            "🚀 Hello 世界 🎉",  // Mixed ASCII and Unicode
        ];

        for test_str in test_cases {
            // Try various truncation lengths
            for max_len in [5, 10, 15, 20, 50] {
                let result = AlertManager::truncate_text(test_str, max_len);

                // Should not panic and should be valid UTF-8
                assert!(
                    result.len() <= max_len,
                    "Truncated string '{}' length {} exceeds max {}",
                    result,
                    result.len(),
                    max_len
                );

                // Should be valid UTF-8
                assert!(
                    result.chars().count() > 0 || result.is_empty(),
                    "Invalid UTF-8 in truncated string: {}",
                    result
                );

                // If truncated, should end with "..."
                if test_str.len() > max_len {
                    assert!(
                        result.ends_with("..."),
                        "Truncated string should end with '...': {}",
                        result
                    );
                }
            }
        }
    }

    #[test]
    fn test_automatic_queue_processing_on_send() {
        let mut manager = AlertManager::new_for_testing_with_queue_size(1, 5);
        let insight = create_test_insight(Severity::Critical, "Critical alert");

        // Fill up the rate limit and queue some alerts
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.current_notification_count(), 1);

        manager.send_alert(&insight).unwrap();
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.queued_alert_count(), 2);

        // Simulate time passing by resetting the rate limiter
        manager.rate_limiter = RateLimiter::new(5);

        // The next send_alert call should automatically process the queue
        manager.send_alert(&insight).unwrap();

        // All queued alerts should be processed automatically
        assert_eq!(manager.queued_alert_count(), 0);
        // Should have processed 2 from queue + 1 new alert = 3 total
        assert_eq!(manager.current_notification_count(), 3);
    }

    #[test]
    fn test_configurable_queue_size() {
        let mut manager = AlertManager::new_for_testing_with_queue_size(1, 2); // Small queue
        let insight = create_test_insight(Severity::Critical, "Critical alert");

        // Fill up the rate limit
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.current_notification_count(), 1);

        // Queue 2 alerts (should fit in queue)
        manager.send_alert(&insight).unwrap();
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.queued_alert_count(), 2);

        // Third queued alert should drop the oldest
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.queued_alert_count(), 2); // Still 2, oldest dropped
    }

    #[test]
    fn test_independent_queue_processing() {
        let mut manager = AlertManager::new_for_testing_with_queue_size(1, 5);
        let insight = create_test_insight(Severity::Critical, "Critical alert");

        // Fill up the rate limit and queue some alerts
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.current_notification_count(), 1);

        manager.send_alert(&insight).unwrap();
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.queued_alert_count(), 2);

        // Simulate time passing by creating a new rate limiter with higher capacity
        let existing_count = manager.current_notification_count();
        let mut new_rate_limiter = RateLimiter::new(5);

        // Manually record the existing notifications in the new rate limiter
        for _ in 0..existing_count {
            new_rate_limiter.record_notification();
        }

        manager.rate_limiter = new_rate_limiter;

        // Process queue independently
        let processed = manager.process_queue().unwrap();
        assert_eq!(processed, 2);
        assert_eq!(manager.queued_alert_count(), 0);

        // Should now have original + processed notifications
        assert_eq!(
            manager.current_notification_count(),
            existing_count + processed
        );
    }

    #[test]
    fn test_autonomous_processing_with_tick() {
        let mut manager = AlertManager::new_for_testing_with_queue_size(1, 5);
        let insight = create_test_insight(Severity::Critical, "Critical alert");

        // Fill up the rate limit and queue some alerts
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.current_notification_count(), 1);

        manager.send_alert(&insight).unwrap();
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.queued_alert_count(), 2);

        // Initially, tick should not process anything due to rate limit
        let processed = manager.tick().unwrap();
        assert_eq!(processed, 0);
        assert_eq!(manager.queued_alert_count(), 2);

        // Simulate time passing by resetting the rate limiter
        manager.rate_limiter = RateLimiter::new(5);

        // Now tick should autonomously process the queue
        let processed = manager.tick().unwrap();
        assert_eq!(processed, 2);
        assert_eq!(manager.queued_alert_count(), 0);
    }

    #[test]
    fn test_has_processable_alerts() {
        let mut manager = AlertManager::new_for_testing_with_queue_size(1, 5);
        let insight = create_test_insight(Severity::Critical, "Critical alert");

        // Initially no processable alerts
        assert!(!manager.has_processable_alerts());

        // Fill up the rate limit and queue some alerts
        manager.send_alert(&insight).unwrap();
        manager.send_alert(&insight).unwrap();
        assert_eq!(manager.queued_alert_count(), 1);

        // Should not be processable due to rate limit
        assert!(!manager.has_processable_alerts());

        // Reset rate limiter to allow processing
        manager.rate_limiter = RateLimiter::new(5);

        // Now should be processable
        assert!(manager.has_processable_alerts());

        // Process the queue
        manager.tick().unwrap();

        // Should no longer be processable (queue empty)
        assert!(!manager.has_processable_alerts());
    }

    #[test]
    fn test_tick_with_empty_queue() {
        let mut manager = AlertManager::new_for_testing(5);

        // Tick with empty queue should do nothing
        let processed = manager.tick().unwrap();
        assert_eq!(processed, 0);
        assert_eq!(manager.queued_alert_count(), 0);
    }

    #[test]
    fn test_tick_respects_rate_limits() {
        let mut manager = AlertManager::new_for_testing_with_queue_size(2, 5);
        let insight = create_test_insight(Severity::Critical, "Critical alert");

        // Queue multiple alerts
        for _ in 0..5 {
            manager.send_alert(&insight).unwrap();
        }

        // Should have sent 2 and queued 3
        assert_eq!(manager.current_notification_count(), 2);
        assert_eq!(manager.queued_alert_count(), 3);

        // Tick should not process more due to rate limit
        let processed = manager.tick().unwrap();
        assert_eq!(processed, 0);
        assert_eq!(manager.queued_alert_count(), 3);

        // Reset rate limiter to allow more
        manager.rate_limiter = RateLimiter::new(5);

        // Now tick should process the remaining queued alerts
        let processed = manager.tick().unwrap();
        assert_eq!(processed, 3);
        assert_eq!(manager.queued_alert_count(), 0);
    }
}

// Property-based tests
#[cfg(test)]
mod property_tests {
    use super::*;
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;

    /// Arbitrary implementation for Severity to generate random severity levels
    impl Arbitrary for Severity {
        fn arbitrary(g: &mut Gen) -> Self {
            let choices = [Severity::Info, Severity::Warning, Severity::Critical];
            *g.choose(&choices).unwrap()
        }
    }

    /// Helper struct to generate valid AI insights for testing
    #[derive(Debug, Clone)]
    struct ValidAIInsight {
        severity: Severity,
        summary: String,
        root_cause: Option<String>,
        recommendations: Vec<String>,
    }

    impl Arbitrary for ValidAIInsight {
        fn arbitrary(g: &mut Gen) -> Self {
            let severity = Severity::arbitrary(g);

            // Generate non-empty summary
            let summary = if String::arbitrary(g).is_empty() {
                "Test summary".to_string()
            } else {
                String::arbitrary(g)
            };

            // Generate optional root cause
            let root_cause = if bool::arbitrary(g) {
                Some(String::arbitrary(g))
            } else {
                None
            };

            // Generate 0-10 recommendations
            let rec_count = usize::arbitrary(g) % 11;
            let recommendations = (0..rec_count)
                .map(|i| format!("Recommendation {}: {}", i + 1, String::arbitrary(g)))
                .collect();

            ValidAIInsight {
                severity,
                summary,
                root_cause,
                recommendations,
            }
        }
    }

    impl ValidAIInsight {
        /// Convert to AIInsight for testing
        fn to_ai_insight(&self) -> AIInsight {
            AIInsight {
                timestamp: chrono::Utc::now(),
                summary: self.summary.clone(),
                root_cause: self.root_cause.clone(),
                recommendations: self.recommendations.clone(),
                evidence: Vec::new(),
                observation_confidence: "unknown".to_string(),
                diagnosis_confidence: "unknown".to_string(),
                limitations: Vec::new(),
                severity: self.severity,
            }
        }
    }

    // Feature: macos-system-observer, Property 12: Critical issues trigger notifications
    // Validates: Requirements 5.1
    #[quickcheck]
    fn prop_critical_issues_trigger_notifications(insight_data: ValidAIInsight) -> bool {
        let mut manager = AlertManager::new_for_testing(10); // High rate limit to avoid interference
        let insight = insight_data.to_ai_insight();

        // Test the property: critical issues should attempt to send notifications
        let result = manager.send_alert(&insight);

        match insight.severity {
            Severity::Critical => {
                // Critical insights should either succeed or be queued
                match result {
                    Ok(()) => {
                        // Should either be sent immediately or queued
                        manager.current_notification_count() > 0 || manager.queued_alert_count() > 0
                    }
                    Err(AlertError::NotificationFailed(_)) => {
                        // Notification failure is acceptable for mock notifications
                        true
                    }
                    _ => false,
                }
            }
            Severity::Warning | Severity::Info => {
                // Non-critical insights should be silently skipped (return Ok but no notification)
                result.is_ok()
                    && manager.current_notification_count() == 0
                    && manager.queued_alert_count() == 0
            }
        }
    }

    // Feature: macos-system-observer, Property 13: Notification content completeness
    // Validates: Requirements 5.2, 5.3
    #[quickcheck]
    fn prop_notification_content_completeness(insight_data: ValidAIInsight) -> bool {
        let insight = insight_data.to_ai_insight();

        // Test the notification formatting logic with truncation
        let title = AlertManager::truncate_text(&format!("System Alert: {}", insight.summary), 256);
        let body =
            AlertManager::truncate_text(&AlertManager::format_notification_body(&insight), 1024);

        // Property: notification title should include the summary (or be truncated properly)
        let title_contains_summary =
            title.contains(&insight.summary) || (title.len() == 256 && title.ends_with("..."));

        // Property: notification body should include root cause if present (or be truncated)
        let body_includes_root_cause = match &insight.root_cause {
            Some(cause) => {
                let trimmed_cause = cause.trim();
                body.contains(trimmed_cause) || (body.len() >= 1021 && body.ends_with("..."))
            }
            None => !body.contains("Cause:") || (body.len() >= 1021 && body.ends_with("...")),
        };

        // Property: notification body should include recommendations if present (or be truncated)
        let body_includes_recommendations = if insight.recommendations.is_empty() {
            !body.contains("Recommendations:") || (body.len() == 1024 && body.ends_with("..."))
        } else {
            // Should contain "Recommendations:" and the content of the first few recommendations
            // or be properly truncated
            body.contains("Recommendations:") || (body.len() == 1024 && body.ends_with("..."))
        };

        // Property: if there are more than 3 recommendations, should indicate this (or be truncated)
        let handles_many_recommendations = if insight.recommendations.len() > 3 {
            body.contains("more recommendations") || (body.len() == 1024 && body.ends_with("..."))
        } else {
            true // No requirement if 3 or fewer
        };

        title_contains_summary
            && body_includes_root_cause
            && body_includes_recommendations
            && handles_many_recommendations
    }

    // Feature: macos-system-observer, Property 14: Notification failures don't halt operation
    // Validates: Requirements 5.4
    #[quickcheck]
    fn prop_notification_failures_dont_halt_operation(insight_data: ValidAIInsight) -> bool {
        let mut manager = AlertManager::new_for_testing(10); // High rate limit to avoid interference
        let insight = insight_data.to_ai_insight();

        // Test that the AlertManager continues to function after any kind of result from send_alert

        // First, try to send the alert
        let first_result = manager.send_alert(&insight);

        // The manager should still be functional regardless of the result
        let manager_still_functional = manager.can_send_notification(); // Should not panic

        // Try to send another alert to verify the manager is still working
        let second_insight = AIInsight {
            timestamp: chrono::Utc::now(),
            summary: "Second test alert".to_string(),
            root_cause: Some("Test cause".to_string()),
            recommendations: vec!["Test recommendation".to_string()],
            evidence: Vec::new(),
            observation_confidence: "unknown".to_string(),
            diagnosis_confidence: "unknown".to_string(),
            limitations: Vec::new(),
            severity: Severity::Critical,
        };

        let second_result = manager.send_alert(&second_insight);

        // Property: The manager should continue to accept alerts regardless of previous failures
        // Both calls should either succeed (return Ok), not return RateLimitExceeded since we queue now
        let first_handled_gracefully = matches!(
            first_result,
            Ok(()) | Err(AlertError::NotificationFailed(_))
        );

        let second_handled_gracefully = matches!(
            second_result,
            Ok(()) | Err(AlertError::NotificationFailed(_))
        );

        manager_still_functional && first_handled_gracefully && second_handled_gracefully
    }

    // Feature: macos-system-observer, Property 15: Rate limiting prevents spam
    // Validates: Requirements 5.5
    #[quickcheck]
    fn prop_rate_limiting_prevents_spam(rate_limit: u8, num_alerts: u8) -> bool {
        // Constrain inputs to reasonable ranges
        let rate_limit = (rate_limit % 10) + 1; // 1-10 alerts per minute
        let num_alerts = (num_alerts % 20) + 1; // 1-20 total alerts to send

        let mut manager = AlertManager::new_for_testing(rate_limit as usize);

        // Create a critical insight that would normally trigger notifications
        let insight = AIInsight {
            timestamp: chrono::Utc::now(),
            summary: "Critical test alert".to_string(),
            root_cause: Some("Test root cause".to_string()),
            recommendations: vec!["Test recommendation".to_string()],
            evidence: Vec::new(),
            observation_confidence: "unknown".to_string(),
            diagnosis_confidence: "unknown".to_string(),
            limitations: Vec::new(),
            severity: Severity::Critical,
        };

        let mut successful_operations = 0;
        let mut failed_sends = 0;

        // Try to send num_alerts alerts
        for _ in 0..num_alerts {
            match manager.send_alert(&insight) {
                Ok(()) => successful_operations += 1, // Either sent or queued
                Err(AlertError::NotificationFailed(_)) => failed_sends += 1,
                _ => return false, // Unexpected error type (no more RateLimitExceeded)
            }
        }

        // Property 1: All operations should succeed (either send or queue)
        let all_operations_handled = (successful_operations + failed_sends) == num_alerts as usize;

        // Property 2: Total sent notifications should not exceed the rate limit
        let sent_count = manager.current_notification_count();
        let respects_rate_limit = sent_count <= rate_limit as usize;

        // Property 3: If we tried to send more than the rate limit, excess should be queued
        let queued_count = manager.queued_alert_count();
        let total_processed = sent_count + queued_count;
        let queueing_works = if num_alerts > rate_limit {
            queued_count > 0 && total_processed <= num_alerts as usize
        } else {
            total_processed <= num_alerts as usize
        };

        all_operations_handled && respects_rate_limit && queueing_works
    }
}
