use crate::ai::AIInsight;
use crate::alerts::RateLimiter;
use crate::error::AlertError;
use crate::events::Severity;
use log::{error, info, warn};
use std::collections::VecDeque;
use std::process::Command;

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
pub struct AlertManager {
    /// Rate limiter to prevent notification spam
    rate_limiter: RateLimiter,
    /// Queue for alerts that are rate limited
    alert_queue: VecDeque<AIInsight>,
    /// Maximum size of the alert queue
    max_queue_size: usize,
    /// Whether to use mock notifications for testing
    use_mock_notifications: bool,
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
        Self {
            rate_limiter: RateLimiter::new(max_per_minute),
            alert_queue: VecDeque::new(),
            max_queue_size,
            use_mock_notifications: false,
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
            use_mock_notifications: true,
        }
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
        // First, automatically process any queued alerts
        self.process_queued_alerts()?;

        // Only send notifications for critical issues by default
        // This can be made configurable in the future
        if insight.severity != Severity::Critical {
            info!(
                "Skipping non-critical notification ({}): {}",
                format!("{:?}", insight.severity).to_lowercase(),
                insight.summary
            );
            return Ok(());
        }

        // Try to send the alert immediately
        if self.rate_limiter.can_send() {
            self.send_notification_now(insight)
        } else {
            // Queue the alert for later processing
            self.queue_alert(insight.clone());
            info!("Queued notification due to rate limit: {}", insight.summary);
            Ok(())
        }
    }

    /// Process queued alerts if rate limiting allows
    fn process_queued_alerts(&mut self) -> Result<(), AlertError> {
        while !self.alert_queue.is_empty() && self.rate_limiter.can_send() {
            if let Some(queued_insight) = self.alert_queue.pop_front() {
                // Only process critical alerts from the queue
                if queued_insight.severity == Severity::Critical {
                    self.send_notification_now(&queued_insight)?;
                }
            }
        }
        Ok(())
    }

    /// Queue an alert for later processing
    fn queue_alert(&mut self, insight: AIInsight) {
        if self.alert_queue.len() >= self.max_queue_size {
            // Drop the oldest alert to make room
            if let Some(dropped) = self.alert_queue.pop_front() {
                warn!(
                    "Alert queue full, dropping oldest alert: {}",
                    dropped.summary
                );
            }
        }
        self.alert_queue.push_back(insight);
    }

    /// Send a notification immediately (assumes rate limit check has passed)
    fn send_notification_now(&mut self, insight: &AIInsight) -> Result<(), AlertError> {
        // Format the notification with truncation
        let title = Self::truncate_text(&format!("System Alert: {}", insight.summary), 256);
        let body = Self::truncate_text(&Self::format_notification_body(insight), 1024);

        // Send the notification
        match self.send_macos_notification(&title, &body) {
            Ok(()) => {
                self.rate_limiter.record_notification();
                info!("Sent notification: {}", insight.summary);
                Ok(())
            }
            Err(e) => {
                error!("Failed to send notification: {}", e);
                Err(e)
            }
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
    fn send_macos_notification(&self, title: &str, body: &str) -> Result<(), AlertError> {
        if self.use_mock_notifications {
            // Mock notification for testing - just log it
            info!("MOCK NOTIFICATION - Title: {}, Body: {}", title, body);
            return Ok(());
        }

        // Escape quotes in title and body for AppleScript
        let escaped_title = title.replace('"', "\\\"");
        let escaped_body = body.replace('"', "\\\"");

        // Create AppleScript command to display notification
        let script = format!(
            r#"display notification "{}" with title "{}""#,
            escaped_body, escaped_title
        );

        // Execute osascript
        let output = Command::new("osascript")
            .arg("-e")
            .arg(&script)
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
        self.rate_limiter.can_send()
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
        !self.alert_queue.is_empty() && self.rate_limiter.can_send()
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
            severity: Severity::Critical,
        };

        let second_result = manager.send_alert(&second_insight);

        // Property: The manager should continue to accept alerts regardless of previous failures
        // Both calls should either succeed (return Ok), not return RateLimitExceeded since we queue now
        let first_handled_gracefully = match first_result {
            Ok(()) | Err(AlertError::NotificationFailed(_)) => true,
            _ => false,
        };

        let second_handled_gracefully = match second_result {
            Ok(()) | Err(AlertError::NotificationFailed(_)) => true,
            _ => false,
        };

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
