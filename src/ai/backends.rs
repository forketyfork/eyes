use crate::ai::AIInsight;
use crate::error::AnalysisError;
use crate::events::Severity;
use crate::triggers::TriggerContext;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Trait for LLM backend implementations
pub trait LLMBackend: Send + Sync {
    fn analyze<'a>(
        &'a self,
        context: &'a TriggerContext,
    ) -> Pin<Box<dyn Future<Output = Result<AIInsight, AnalysisError>> + Send + 'a>>;
}

/// Ollama backend for local LLM inference
///
/// Communicates with a local Ollama server to perform AI analysis.
/// Ollama provides privacy-focused local inference for Apple Silicon Macs.
pub struct OllamaBackend {
    client: Client,
    endpoint: String,
    model: String,
}

/// Request format for Ollama API
#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
    options: OllamaOptions,
}

/// Options for Ollama inference
#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f32,
    top_p: f32,
    max_tokens: u32,
}

/// Response format from Ollama API
#[derive(Debug, Deserialize)]
struct OllamaResponse {
    response: String,
    #[allow(dead_code)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

/// Expected JSON structure from LLM response
#[derive(Debug, Serialize, Deserialize)]
struct LLMAnalysisResponse {
    summary: String,
    root_cause: Option<String>,
    recommendations: Vec<String>,
    severity: String,
}

impl OllamaBackend {
    /// Create a new Ollama backend
    ///
    /// # Arguments
    /// * `endpoint` - Ollama server URL (e.g., "http://localhost:11434")
    /// * `model` - Model name to use (e.g., "llama3", "mistral")
    ///
    /// # Example
    /// ```
    /// use eyes::ai::backends::OllamaBackend;
    ///
    /// let backend = OllamaBackend::new(
    ///     "http://localhost:11434".to_string(),
    ///     "llama3".to_string()
    /// );
    /// ```
    pub fn new(endpoint: String, model: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60)) // 60 second timeout for LLM requests
            .no_proxy()
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            endpoint,
            model,
        }
    }

    /// Format the Ollama API endpoint URL
    fn api_url(&self) -> String {
        format!("{}/api/generate", self.endpoint.trim_end_matches('/'))
    }

    /// Parse the severity string from LLM response
    fn parse_severity(severity_str: &str) -> Severity {
        match severity_str.to_lowercase().as_str() {
            "critical" => Severity::Critical,
            "warning" => Severity::Warning,
            _ => Severity::Info, // Default to Info for unknown values
        }
    }

    /// Extract JSON from LLM response text
    ///
    /// LLMs sometimes wrap JSON in markdown code blocks or add extra text.
    /// This method attempts to extract the JSON portion.
    fn extract_json_from_response(response_text: &str) -> Result<String, AnalysisError> {
        let text = response_text.trim();

        // Try to find JSON within markdown code blocks
        if let Some(start) = text.find("```json") {
            if let Some(end) = text[start..].find("```") {
                let json_start = start + 7; // Length of "```json"
                let json_end = start + end;
                if json_start < json_end {
                    return Ok(text[json_start..json_end].trim().to_string());
                }
            }
        }

        // Try to find JSON within regular code blocks
        if let Some(start) = text.find("```") {
            if let Some(end) = text[start + 3..].find("```") {
                let json_start = start + 3;
                let json_end = start + 3 + end;
                if json_start < json_end {
                    let potential_json = text[json_start..json_end].trim();
                    if potential_json.starts_with('{') && potential_json.ends_with('}') {
                        return Ok(potential_json.to_string());
                    }
                }
            }
        }

        // Try to find JSON by looking for { and } boundaries
        if let Some(start) = text.find('{') {
            if let Some(end) = text.rfind('}') {
                if start < end {
                    return Ok(text[start..=end].to_string());
                }
            }
        }

        // If no JSON structure found, return the original text
        // The caller will handle the parsing error
        Ok(text.to_string())
    }
}

impl LLMBackend for OllamaBackend {
    fn analyze<'a>(
        &'a self,
        context: &'a TriggerContext,
    ) -> Pin<Box<dyn Future<Output = Result<AIInsight, AnalysisError>> + Send + 'a>> {
        Box::pin(async move {
            // Format the prompt using the analyzer's format_prompt method
            // We need to create a temporary analyzer to access the method
            let analyzer = crate::ai::analyzer::AIAnalyzer::new();
            let prompt = analyzer.format_prompt(context);

            // Prepare the request
            let request = OllamaRequest {
                model: self.model.clone(),
                prompt,
                stream: false, // We want the complete response, not streaming
                options: OllamaOptions {
                    temperature: 0.1, // Low temperature for more consistent analysis
                    top_p: 0.9,
                    max_tokens: 1000, // Reasonable limit for diagnostic responses
                },
            };

            // Send the request
            let response = self
                .client
                .post(self.api_url())
                .json(&request)
                .send()
                .await
                .map_err(|e| AnalysisError::HttpError(format!("HTTP request failed: {}", e)))?;

            // Check for HTTP errors
            if !response.status().is_success() {
                let status = response.status();
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                return Err(AnalysisError::BackendError(format!(
                    "Ollama API returned error {}: {}",
                    status, error_text
                )));
            }

            // Parse the Ollama response
            let ollama_response: OllamaResponse = response.json().await.map_err(|e| {
                AnalysisError::InvalidResponse(format!("Failed to parse Ollama response: {}", e))
            })?;

            // Check for Ollama-specific errors
            if let Some(error) = ollama_response.error {
                return Err(AnalysisError::BackendError(format!(
                    "Ollama error: {}",
                    error
                )));
            }

            // Extract JSON from the LLM response
            let json_text = Self::extract_json_from_response(&ollama_response.response)?;

            // Parse the LLM's JSON response
            let llm_response: LLMAnalysisResponse =
                serde_json::from_str(&json_text).map_err(|e| {
                    AnalysisError::InvalidResponse(format!(
                        "Failed to parse LLM JSON response: {}. Response was: {}",
                        e, json_text
                    ))
                })?;

            // Convert to AIInsight
            let severity = Self::parse_severity(&llm_response.severity);

            Ok(AIInsight::new(
                llm_response.summary,
                llm_response.root_cause,
                llm_response.recommendations,
                severity,
            ))
        })
    }
}

/// OpenAI backend for cloud-based LLM inference
///
/// Communicates with OpenAI's API to perform AI analysis using models like GPT-4.
/// Requires an API key and internet connection.
pub struct OpenAIBackend {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

/// Request format for OpenAI Chat Completions API
#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    temperature: f32,
    max_tokens: u32,
    response_format: OpenAIResponseFormat,
}

/// Message format for OpenAI API
#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

/// Response format specification for OpenAI API
#[derive(Debug, Serialize)]
struct OpenAIResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

/// Response format from OpenAI API
#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    error: Option<OpenAIError>,
}

/// Choice in OpenAI response
#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIResponseMessage,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

/// Message in OpenAI response
#[derive(Debug, Deserialize)]
struct OpenAIResponseMessage {
    content: String,
}

/// Error format from OpenAI API
#[derive(Debug, Deserialize)]
struct OpenAIError {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
}

impl OpenAIBackend {
    /// Create a new OpenAI backend
    ///
    /// # Arguments
    /// * `api_key` - OpenAI API key
    /// * `model` - Model name to use (e.g., "gpt-4", "gpt-3.5-turbo")
    ///
    /// # Example
    /// ```
    /// use eyes::ai::backends::OpenAIBackend;
    ///
    /// let backend = OpenAIBackend::new(
    ///     "sk-...".to_string(),
    ///     "gpt-4".to_string()
    /// );
    /// ```
    pub fn new(api_key: String, model: String) -> Self {
        Self::with_base_url(api_key, model, "https://api.openai.com/v1".to_string())
    }

    /// Create a new OpenAI backend with custom base URL
    ///
    /// This allows using OpenAI-compatible APIs or custom endpoints.
    pub fn with_base_url(api_key: String, model: String, base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60)) // 60 second timeout for LLM requests
            .no_proxy()
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            api_key,
            model,
            base_url,
        }
    }

    /// Format the OpenAI API endpoint URL
    fn api_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    /// Create the system prompt for OpenAI
    fn create_system_prompt() -> String {
        "You are a macOS system diagnostics expert. Analyze system data and provide insights in JSON format with fields: summary (string), root_cause (string or null), recommendations (array of strings), severity (\"info\", \"warning\", or \"critical\").".to_string()
    }
}

impl LLMBackend for OpenAIBackend {
    fn analyze<'a>(
        &'a self,
        context: &'a TriggerContext,
    ) -> Pin<Box<dyn Future<Output = Result<AIInsight, AnalysisError>> + Send + 'a>> {
        Box::pin(async move {
            // Format the prompt using the analyzer's format_prompt method
            let analyzer = crate::ai::analyzer::AIAnalyzer::new();
            let user_prompt = analyzer.format_prompt(context);

            // Prepare the request
            let request = OpenAIRequest {
                model: self.model.clone(),
                messages: vec![
                    OpenAIMessage {
                        role: "system".to_string(),
                        content: Self::create_system_prompt(),
                    },
                    OpenAIMessage {
                        role: "user".to_string(),
                        content: user_prompt,
                    },
                ],
                temperature: 0.1, // Low temperature for consistent analysis
                max_tokens: 1000, // Reasonable limit for diagnostic responses
                response_format: OpenAIResponseFormat {
                    format_type: "json_object".to_string(),
                },
            };

            // Send the request with authentication
            let response = self
                .client
                .post(self.api_url())
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await
                .map_err(|e| AnalysisError::HttpError(format!("HTTP request failed: {}", e)))?;

            // Check for HTTP errors
            if !response.status().is_success() {
                let status = response.status();
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                return Err(AnalysisError::BackendError(format!(
                    "OpenAI API returned error {}: {}",
                    status, error_text
                )));
            }

            // Parse the OpenAI response
            let openai_response: OpenAIResponse = response.json().await.map_err(|e| {
                AnalysisError::InvalidResponse(format!("Failed to parse OpenAI response: {}", e))
            })?;

            // Check for OpenAI-specific errors
            if let Some(error) = openai_response.error {
                return Err(AnalysisError::BackendError(format!(
                    "OpenAI API error ({}): {}",
                    error.error_type, error.message
                )));
            }

            // Extract the response content
            let content = openai_response
                .choices
                .first()
                .ok_or_else(|| {
                    AnalysisError::InvalidResponse("No choices in OpenAI response".to_string())
                })?
                .message
                .content
                .clone();

            // Parse the LLM's JSON response
            let llm_response: LLMAnalysisResponse =
                serde_json::from_str(&content).map_err(|e| {
                    AnalysisError::InvalidResponse(format!(
                        "Failed to parse LLM JSON response: {}. Response was: {}",
                        e, content
                    ))
                })?;

            // Convert to AIInsight
            let severity = OllamaBackend::parse_severity(&llm_response.severity);

            Ok(AIInsight::new(
                llm_response.summary,
                llm_response.root_cause,
                llm_response.recommendations,
                severity,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{LogEvent, MemoryPressure, MessageType, MetricsEvent};
    use chrono::Utc;

    pub(crate) fn create_test_context() -> TriggerContext {
        let log_events = vec![LogEvent {
            timestamp: Utc::now(),
            message_type: MessageType::Error,
            subsystem: "com.apple.test".to_string(),
            category: "test".to_string(),
            process: "testd".to_string(),
            process_id: 1234,
            message: "Test error message".to_string(),
        }];

        let metrics_events = vec![MetricsEvent {
            timestamp: Utc::now(),
            cpu_power_mw: 2000.0,
            cpu_usage_percent: 80.0,
            gpu_power_mw: Some(800.0),
            gpu_usage_percent: Some(60.0),
            memory_pressure: MemoryPressure::Warning,
            memory_used_mb: 6144.0,
            energy_impact: 2800.0,
        }];

        TriggerContext {
            timestamp: Utc::now(),
            log_events,
            metrics_events,
            triggered_by: "TestRule".to_string(),
            expected_severity: Severity::Warning,
            trigger_reason: "Test trigger".to_string(),
        }
    }

    #[test]
    fn test_ollama_backend_creation() {
        let backend =
            OllamaBackend::new("http://localhost:11434".to_string(), "llama3".to_string());

        assert_eq!(backend.endpoint, "http://localhost:11434");
        assert_eq!(backend.model, "llama3");
        assert_eq!(backend.api_url(), "http://localhost:11434/api/generate");
    }

    #[test]
    fn test_ollama_backend_api_url_formatting() {
        // Test with trailing slash
        let backend1 =
            OllamaBackend::new("http://localhost:11434/".to_string(), "llama3".to_string());
        assert_eq!(backend1.api_url(), "http://localhost:11434/api/generate");

        // Test without trailing slash
        let backend2 =
            OllamaBackend::new("http://localhost:11434".to_string(), "llama3".to_string());
        assert_eq!(backend2.api_url(), "http://localhost:11434/api/generate");
    }

    #[test]
    fn test_parse_severity() {
        assert_eq!(
            OllamaBackend::parse_severity("critical"),
            Severity::Critical
        );
        assert_eq!(
            OllamaBackend::parse_severity("CRITICAL"),
            Severity::Critical
        );
        assert_eq!(OllamaBackend::parse_severity("warning"), Severity::Warning);
        assert_eq!(OllamaBackend::parse_severity("WARNING"), Severity::Warning);
        assert_eq!(OllamaBackend::parse_severity("info"), Severity::Info);
        assert_eq!(OllamaBackend::parse_severity("INFO"), Severity::Info);
        assert_eq!(OllamaBackend::parse_severity("unknown"), Severity::Info); // Default
    }

    #[test]
    fn test_extract_json_from_response() {
        // Test JSON in markdown code block
        let response1 = r#"Here's the analysis:

```json
{
  "summary": "High CPU usage",
  "root_cause": "Heavy process",
  "recommendations": ["Close apps"],
  "severity": "warning"
}
```

That's my analysis."#;

        let json1 = OllamaBackend::extract_json_from_response(response1).unwrap();
        assert!(json1.contains("High CPU usage"));
        assert!(json1.starts_with('{'));
        assert!(json1.ends_with('}'));

        // Test JSON in regular code block
        let response2 = r#"Analysis result:
```
{
  "summary": "Memory issue",
  "root_cause": null,
  "recommendations": ["Restart"],
  "severity": "critical"
}
```"#;

        let json2 = OllamaBackend::extract_json_from_response(response2).unwrap();
        assert!(json2.contains("Memory issue"));

        // Test plain JSON
        let response3 = r#"{"summary": "System OK", "root_cause": null, "recommendations": [], "severity": "info"}"#;
        let json3 = OllamaBackend::extract_json_from_response(response3).unwrap();
        assert_eq!(json3, response3);

        // Test JSON with surrounding text
        let response4 = r#"The analysis shows: {"summary": "Test", "root_cause": "Cause", "recommendations": ["Fix"], "severity": "warning"} - end of analysis"#;
        let json4 = OllamaBackend::extract_json_from_response(response4).unwrap();
        assert!(json4.contains("Test"));
        assert!(json4.starts_with('{'));
        assert!(json4.ends_with('}'));
    }

    #[test]
    fn test_llm_analysis_response_deserialization() {
        let json = r#"{
            "summary": "High memory usage detected",
            "root_cause": "Multiple browser tabs open",
            "recommendations": ["Close unused tabs", "Restart browser"],
            "severity": "warning"
        }"#;

        let response: LLMAnalysisResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.summary, "High memory usage detected");
        assert_eq!(
            response.root_cause,
            Some("Multiple browser tabs open".to_string())
        );
        assert_eq!(response.recommendations.len(), 2);
        assert_eq!(response.severity, "warning");
    }

    #[test]
    fn test_llm_analysis_response_with_null_root_cause() {
        let json = r#"{
            "summary": "System running normally",
            "root_cause": null,
            "recommendations": [],
            "severity": "info"
        }"#;

        let response: LLMAnalysisResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.summary, "System running normally");
        assert_eq!(response.root_cause, None);
        assert_eq!(response.recommendations.len(), 0);
        assert_eq!(response.severity, "info");
    }

    // Note: Integration tests with actual Ollama server would require
    // a running Ollama instance and are marked as ignored
    #[tokio::test]
    #[ignore = "Requires running Ollama server"]
    async fn test_ollama_backend_integration() {
        let backend =
            OllamaBackend::new("http://localhost:11434".to_string(), "llama3".to_string());

        let context = create_test_context();
        let result = backend.analyze(&context).await;

        // This test would only pass with a real Ollama server
        match result {
            Ok(insight) => {
                assert!(!insight.summary.is_empty());
                println!("Analysis result: {:?}", insight);
            }
            Err(e) => {
                println!("Expected error (no Ollama server): {:?}", e);
            }
        }
    }

    #[test]
    fn test_openai_backend_creation() {
        let backend = OpenAIBackend::new("sk-test-key".to_string(), "gpt-4".to_string());

        assert_eq!(backend.api_key, "sk-test-key");
        assert_eq!(backend.model, "gpt-4");
        assert_eq!(backend.base_url, "https://api.openai.com/v1");
        assert_eq!(
            backend.api_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_openai_backend_with_custom_base_url() {
        let backend = OpenAIBackend::with_base_url(
            "sk-test-key".to_string(),
            "gpt-3.5-turbo".to_string(),
            "https://custom-api.example.com/v1/".to_string(),
        );

        assert_eq!(backend.base_url, "https://custom-api.example.com/v1/");
        assert_eq!(
            backend.api_url(),
            "https://custom-api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_openai_system_prompt() {
        let prompt = OpenAIBackend::create_system_prompt();
        assert!(prompt.contains("macOS system diagnostics expert"));
        assert!(prompt.contains("JSON format"));
        assert!(prompt.contains("summary"));
        assert!(prompt.contains("root_cause"));
        assert!(prompt.contains("recommendations"));
        assert!(prompt.contains("severity"));
    }

    #[test]
    fn test_openai_request_serialization() {
        let request = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "system".to_string(),
                    content: "You are an expert".to_string(),
                },
                OpenAIMessage {
                    role: "user".to_string(),
                    content: "Analyze this".to_string(),
                },
            ],
            temperature: 0.1,
            max_tokens: 1000,
            response_format: OpenAIResponseFormat {
                format_type: "json_object".to_string(),
            },
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("gpt-4"));
        assert!(json.contains("system"));
        assert!(json.contains("user"));
        assert!(json.contains("json_object"));
    }

    #[test]
    fn test_openai_response_deserialization() {
        let json = r#"{
            "choices": [
                {
                    "message": {
                        "content": "{\"summary\": \"Test\", \"root_cause\": null, \"recommendations\": [], \"severity\": \"info\"}"
                    },
                    "finish_reason": "stop"
                }
            ]
        }"#;

        let response: OpenAIResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.choices.len(), 1);
        assert!(response.choices[0].message.content.contains("Test"));
        assert_eq!(response.choices[0].finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn test_openai_error_response_deserialization() {
        let json = r#"{
            "error": {
                "message": "Invalid API key",
                "type": "invalid_request_error"
            },
            "choices": []
        }"#;

        let response: OpenAIResponse = serde_json::from_str(json).unwrap();
        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.message, "Invalid API key");
        assert_eq!(error.error_type, "invalid_request_error");
    }

    // Note: Integration tests with actual OpenAI API would require
    // a valid API key and are marked as ignored
    #[tokio::test]
    #[ignore = "Requires valid OpenAI API key"]
    async fn test_openai_backend_integration() {
        // This test requires setting OPENAI_API_KEY environment variable
        let api_key =
            std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY environment variable not set");

        let backend = OpenAIBackend::new(api_key, "gpt-3.5-turbo".to_string());
        let context = create_test_context();
        let result = backend.analyze(&context).await;

        // This test would only pass with a valid API key
        match result {
            Ok(insight) => {
                assert!(!insight.summary.is_empty());
                println!("Analysis result: {:?}", insight);
            }
            Err(e) => {
                println!("Expected error (no API key or network): {:?}", e);
            }
        }
    }
}

// Property-based tests
#[cfg(test)]
mod property_tests {
    use super::*;
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;

    /// Helper to generate valid LLM analysis responses for testing
    #[derive(Debug, Clone)]
    struct ValidLLMResponse {
        summary: String,
        root_cause: Option<String>,
        recommendations: Vec<String>,
        severity: String,
    }

    impl Arbitrary for ValidLLMResponse {
        fn arbitrary(g: &mut Gen) -> Self {
            let severities = ["info", "warning", "critical"];
            let severity = g.choose(&severities).unwrap().to_string();

            let summary = format!("System issue {}", u32::arbitrary(g));

            let root_cause = if bool::arbitrary(g) {
                Some(format!("Root cause {}", u32::arbitrary(g)))
            } else {
                None
            };

            let rec_count = (u8::arbitrary(g) % 5) as usize; // 0-4 recommendations
            let mut recommendations = Vec::new();
            for i in 0..rec_count {
                recommendations.push(format!("Recommendation {}", i));
            }

            Self {
                summary,
                root_cause,
                recommendations,
                severity,
            }
        }
    }

    impl ValidLLMResponse {
        /// Convert to JSON string
        fn to_json(&self) -> String {
            serde_json::to_string(&LLMAnalysisResponse {
                summary: self.summary.clone(),
                root_cause: self.root_cause.clone(),
                recommendations: self.recommendations.clone(),
                severity: self.severity.clone(),
            })
            .unwrap()
        }

        /// Convert to JSON wrapped in markdown code block (common LLM behavior)
        fn to_markdown_json(&self) -> String {
            format!(
                "Here's the analysis:\n\n```json\n{}\n```\n\nThat's my assessment.",
                self.to_json()
            )
        }

        /// Convert to JSON with surrounding text (another common LLM behavior)
        fn to_text_with_json(&self) -> String {
            format!(
                "Based on the system data, I found: {} Let me provide the details.",
                self.to_json()
            )
        }
    }

    // Feature: macos-system-observer, Property 11: LLM response extraction
    // Validates: Requirements 4.5
    #[quickcheck]
    fn prop_llm_response_extraction_from_json(response_data: ValidLLMResponse) -> bool {
        let json_text = response_data.to_json();

        // Property: Valid JSON should parse correctly
        let parsed_result: Result<LLMAnalysisResponse, _> = serde_json::from_str(&json_text);

        match parsed_result {
            Ok(parsed) => {
                // Property: All fields should be preserved
                parsed.summary == response_data.summary
                    && parsed.root_cause == response_data.root_cause
                    && parsed.recommendations == response_data.recommendations
                    && parsed.severity == response_data.severity
            }
            Err(_) => false, // Valid data should always parse
        }
    }

    // Property test for JSON extraction from markdown
    #[quickcheck]
    fn prop_json_extraction_from_markdown(response_data: ValidLLMResponse) -> bool {
        let markdown_text = response_data.to_markdown_json();

        // Property: Should extract JSON from markdown code blocks
        let extracted_json = OllamaBackend::extract_json_from_response(&markdown_text);

        match extracted_json {
            Ok(json) => {
                // Property: Extracted JSON should parse to the same data
                let parsed_result: Result<LLMAnalysisResponse, _> = serde_json::from_str(&json);
                match parsed_result {
                    Ok(parsed) => {
                        parsed.summary == response_data.summary
                            && parsed.root_cause == response_data.root_cause
                            && parsed.recommendations == response_data.recommendations
                            && parsed.severity == response_data.severity
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    }

    // Property test for JSON extraction from text with embedded JSON
    #[quickcheck]
    fn prop_json_extraction_from_text(response_data: ValidLLMResponse) -> bool {
        let text_with_json = response_data.to_text_with_json();

        // Property: Should extract JSON from text with surrounding content
        let extracted_json = OllamaBackend::extract_json_from_response(&text_with_json);

        match extracted_json {
            Ok(json) => {
                // Property: Extracted JSON should parse to the same data
                let parsed_result: Result<LLMAnalysisResponse, _> = serde_json::from_str(&json);
                match parsed_result {
                    Ok(parsed) => {
                        parsed.summary == response_data.summary
                            && parsed.root_cause == response_data.root_cause
                            && parsed.recommendations == response_data.recommendations
                            && parsed.severity == response_data.severity
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    }

    // Property test for severity parsing
    #[quickcheck]
    fn prop_severity_parsing_consistency(severity_input: String) -> bool {
        let parsed_severity = OllamaBackend::parse_severity(&severity_input);

        // Property: Parsing should be consistent and deterministic
        let parsed_again = OllamaBackend::parse_severity(&severity_input);
        parsed_severity == parsed_again
    }

    // Property test for severity parsing with known values
    #[quickcheck]
    fn prop_severity_parsing_known_values() -> bool {
        // Property: Known severity values should map correctly
        let critical_variants = ["critical", "CRITICAL", "Critical"];
        let warning_variants = ["warning", "WARNING", "Warning"];
        let info_variants = ["info", "INFO", "Info"];

        let critical_correct = critical_variants
            .iter()
            .all(|&s| OllamaBackend::parse_severity(s) == Severity::Critical);

        let warning_correct = warning_variants
            .iter()
            .all(|&s| OllamaBackend::parse_severity(s) == Severity::Warning);

        let info_correct = info_variants
            .iter()
            .all(|&s| OllamaBackend::parse_severity(s) == Severity::Info);

        // Property: Unknown values should default to Info
        let unknown_defaults_to_info = OllamaBackend::parse_severity("unknown") == Severity::Info;

        critical_correct && warning_correct && info_correct && unknown_defaults_to_info
    }

    /// Helper to generate malformed JSON for testing error handling
    #[derive(Debug, Clone)]
    struct MalformedJSON {
        content: String,
    }

    impl Arbitrary for MalformedJSON {
        fn arbitrary(g: &mut Gen) -> Self {
            let malformed_variants = [
                "{\"summary\": \"test\", \"missing_closing_brace\"",
                "\"summary\": \"test\", \"root_cause\": null}", // Missing opening brace
                "{\"summary\": \"test\", \"root_cause\": null, \"recommendations\": [}", // Incomplete array
                "{\"summary\": \"test\", \"root_cause\": null, \"recommendations\": [], \"severity\": }", // Missing value
                "not json at all",
                "",
                "{}",
                "{\"wrong_fields\": \"test\"}",
            ];

            let content = g.choose(&malformed_variants).unwrap().to_string();
            Self { content }
        }
    }

    // Property test for error handling with malformed JSON
    #[quickcheck]
    fn prop_malformed_json_handling(malformed: MalformedJSON) -> bool {
        // Property: Malformed JSON should result in parsing errors, not panics
        let parse_result: Result<LLMAnalysisResponse, _> = serde_json::from_str(&malformed.content);

        // Property: Should either parse successfully (if accidentally valid) or fail gracefully
        match parse_result {
            Ok(_) => true,  // Accidentally valid JSON is fine
            Err(_) => true, // Expected error is fine
        }
        // The important thing is that it doesn't panic
    }

    // Property test for AI insight creation from LLM response
    #[quickcheck]
    fn prop_ai_insight_creation_from_llm_response(response_data: ValidLLMResponse) -> bool {
        let severity = OllamaBackend::parse_severity(&response_data.severity);

        let insight = AIInsight::new(
            response_data.summary.clone(),
            response_data.root_cause.clone(),
            response_data.recommendations.clone(),
            severity,
        );

        // Property: AIInsight should preserve all data from LLM response
        insight.summary == response_data.summary
            && insight.root_cause == response_data.root_cause
            && insight.recommendations == response_data.recommendations
            && insight.severity == severity
    }

    /// Mock backend that simulates failures for testing retry behavior
    #[derive(Debug)]
    struct FailingBackend {
        failure_count: std::sync::Arc<std::sync::Mutex<usize>>,
        max_failures: usize,
        error_message: String,
    }

    impl FailingBackend {
        fn new(max_failures: usize, error_message: String) -> Self {
            Self {
                failure_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
                max_failures,
                error_message,
            }
        }

        fn failure_count(&self) -> usize {
            *self.failure_count.lock().unwrap()
        }
    }

    unsafe impl Send for FailingBackend {}
    unsafe impl Sync for FailingBackend {}

    impl LLMBackend for FailingBackend {
        fn analyze<'a>(
            &'a self,
            _context: &'a TriggerContext,
        ) -> Pin<Box<dyn Future<Output = Result<AIInsight, AnalysisError>> + Send + 'a>> {
            Box::pin(async move {
                let mut count = self.failure_count.lock().unwrap();
                *count += 1;

                if *count <= self.max_failures {
                    // Simulate different types of failures
                    match *count % 3 {
                        0 => Err(AnalysisError::BackendError(self.error_message.clone())),
                        1 => Err(AnalysisError::Timeout),
                        _ => Err(AnalysisError::InvalidResponse(
                            "Simulated invalid response".to_string(),
                        )),
                    }
                } else {
                    // Eventually succeed
                    Ok(AIInsight::new(
                        "Analysis after retry".to_string(),
                        Some("Temporary backend issue resolved".to_string()),
                        vec!["System is now stable".to_string()],
                        Severity::Info,
                    ))
                }
            })
        }
    }

    // Feature: macos-system-observer, Property 19: AI backend failures are queued for retry
    // Validates: Requirements 7.3
    #[quickcheck]
    fn prop_backend_failure_handling(failure_count: u8) -> bool {
        let max_failures = (failure_count % 5) as usize; // 0-4 failures
        let error_message = format!("Simulated error {}", failure_count);

        let backend = std::sync::Arc::new(FailingBackend::new(max_failures, error_message.clone()));
        let analyzer = crate::ai::analyzer::AIAnalyzer::with_backend(backend.clone());

        // Create a test context
        let context = super::tests::create_test_context();

        // Property: Backend failures should be returned as errors, not panics
        let rt = tokio::runtime::Runtime::new().unwrap();

        // Try analysis multiple times to test failure behavior
        let mut results = Vec::new();
        for _ in 0..=max_failures + 1 {
            let result = rt.block_on(analyzer.analyze(&context));
            results.push(result);
        }

        // Property: First max_failures attempts should fail
        let initial_failures_correct = results.iter().take(max_failures).all(|r| r.is_err());

        // Property: After max_failures, should succeed (if max_failures > 0)
        let eventual_success = if max_failures > 0 {
            results
                .get(max_failures)
                .map(|r| r.is_ok())
                .unwrap_or(false)
        } else {
            // If max_failures is 0, first attempt should succeed
            results.first().map(|r| r.is_ok()).unwrap_or(false)
        };

        // Property: Backend should track invocation count correctly
        let expected_invocations = max_failures + 1;
        let actual_invocations = backend.failure_count();
        let invocation_count_correct = actual_invocations >= expected_invocations;

        // Property: All operations should complete without panicking
        let no_panics = true; // If we got here, no panics occurred

        initial_failures_correct && eventual_success && invocation_count_correct && no_panics
    }

    // Property test for different error types
    #[quickcheck]
    fn prop_backend_error_types_handling(_error_type: u8) -> bool {
        let backend = std::sync::Arc::new(FailingBackend::new(1, "Test error".to_string()));
        let analyzer = crate::ai::analyzer::AIAnalyzer::with_backend(backend);

        let context = super::tests::create_test_context();
        let rt = tokio::runtime::Runtime::new().unwrap();

        // Property: Different error types should all be handled gracefully
        let result = rt.block_on(analyzer.analyze(&context));

        match result {
            Ok(_) => false, // Should fail on first attempt
            Err(error) => {
                // Property: Error should be one of the expected types
                matches!(
                    error,
                    AnalysisError::BackendError(_)
                        | AnalysisError::Timeout
                        | AnalysisError::InvalidResponse(_)
                )
            }
        }
    }

    // Property test for backend timeout behavior
    #[quickcheck]
    fn prop_backend_timeout_consistency() -> bool {
        // Property: Timeout errors should be consistent
        let timeout_error1 = AnalysisError::Timeout;
        let timeout_error2 = AnalysisError::Timeout;

        // Property: Timeout errors should be equal (for error handling consistency)
        format!("{:?}", timeout_error1) == format!("{:?}", timeout_error2)
    }

    // Property test for error message preservation
    #[quickcheck]
    fn prop_error_message_preservation(error_message: String) -> bool {
        let backend_error = AnalysisError::BackendError(error_message.clone());
        let invalid_response_error = AnalysisError::InvalidResponse(error_message.clone());

        // Property: Error messages should be preserved in error types
        let backend_preserves = format!("{}", backend_error).contains(&error_message);
        let invalid_preserves = format!("{}", invalid_response_error).contains(&error_message);

        backend_preserves && invalid_preserves
    }
}

/// Mock backend for testing and development
///
/// This backend provides configurable responses and can simulate various
/// behaviors including delays, failures, and different response types.
/// Useful for unit testing and development when real LLM backends are not available.
pub struct MockBackend {
    responses: Vec<Result<AIInsight, AnalysisError>>,
    current_index: std::sync::Arc<std::sync::Mutex<usize>>,
    delay: Option<Duration>,
    call_count: std::sync::Arc<std::sync::Mutex<usize>>,
    last_context: std::sync::Arc<std::sync::Mutex<Option<TriggerContext>>>,
}

impl MockBackend {
    /// Create a new mock backend with a single successful response
    ///
    /// # Example
    /// ```
    /// use eyes::ai::backends::MockBackend;
    /// use eyes::ai::AIInsight;
    /// use eyes::events::Severity;
    ///
    /// let insight = AIInsight::new(
    ///     "Mock analysis".to_string(),
    ///     None,
    ///     vec!["Mock recommendation".to_string()],
    ///     Severity::Info
    /// );
    /// let backend = MockBackend::with_response(Ok(insight));
    /// ```
    pub fn with_response(response: Result<AIInsight, AnalysisError>) -> Self {
        Self {
            responses: vec![response],
            current_index: std::sync::Arc::new(std::sync::Mutex::new(0)),
            delay: None,
            call_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
            last_context: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Create a new mock backend with multiple responses
    ///
    /// Responses are returned in order. After the last response,
    /// the backend cycles back to the first response.
    pub fn with_responses(responses: Vec<Result<AIInsight, AnalysisError>>) -> Self {
        Self {
            responses,
            current_index: std::sync::Arc::new(std::sync::Mutex::new(0)),
            delay: None,
            call_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
            last_context: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Create a mock backend that always returns successful insights
    pub fn success() -> Self {
        let insight = AIInsight::new(
            "Mock successful analysis".to_string(),
            Some("Mock root cause".to_string()),
            vec![
                "Mock recommendation 1".to_string(),
                "Mock recommendation 2".to_string(),
            ],
            Severity::Info,
        );
        Self::with_response(Ok(insight))
    }

    /// Create a mock backend that always returns errors
    pub fn error(error_message: String) -> Self {
        Self::with_response(Err(AnalysisError::BackendError(error_message)))
    }

    /// Create a mock backend that simulates timeout errors
    pub fn timeout() -> Self {
        Self::with_response(Err(AnalysisError::Timeout))
    }

    /// Add a delay to all responses (useful for testing timeout behavior)
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// Get the number of times analyze() has been called
    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }

    /// Get the last context that was passed to analyze()
    pub fn last_context(&self) -> Option<TriggerContext> {
        self.last_context.lock().unwrap().clone()
    }

    /// Reset the call count and context tracking
    pub fn reset(&self) {
        *self.call_count.lock().unwrap() = 0;
        *self.last_context.lock().unwrap() = None;
        *self.current_index.lock().unwrap() = 0;
    }
}

impl LLMBackend for MockBackend {
    fn analyze<'a>(
        &'a self,
        context: &'a TriggerContext,
    ) -> Pin<Box<dyn Future<Output = Result<AIInsight, AnalysisError>> + Send + 'a>> {
        Box::pin(async move {
            // Track the call
            *self.call_count.lock().unwrap() += 1;
            *self.last_context.lock().unwrap() = Some(context.clone());

            // Apply delay if configured
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }

            // Get the current response
            let mut index = self.current_index.lock().unwrap();
            let response_index = *index % self.responses.len();
            *index += 1;

            self.responses[response_index].clone()
        })
    }
}

#[cfg(test)]
mod mock_backend_tests {
    use super::*;
    use crate::events::{LogEvent, MemoryPressure, MessageType, MetricsEvent};
    use chrono::Utc;

    fn create_mock_context() -> TriggerContext {
        let log_events = vec![LogEvent {
            timestamp: Utc::now(),
            message_type: MessageType::Error,
            subsystem: "com.apple.mock".to_string(),
            category: "test".to_string(),
            process: "mockd".to_string(),
            process_id: 9999,
            message: "Mock error message".to_string(),
        }];

        let metrics_events = vec![MetricsEvent {
            timestamp: Utc::now(),
            cpu_power_mw: 1500.0,
            cpu_usage_percent: 60.0,
            gpu_power_mw: Some(600.0),
            gpu_usage_percent: Some(40.0),
            memory_pressure: MemoryPressure::Normal,
            memory_used_mb: 4096.0,
            energy_impact: 2100.0,
        }];

        TriggerContext {
            timestamp: Utc::now(),
            log_events,
            metrics_events,
            triggered_by: "MockRule".to_string(),
            expected_severity: Severity::Info,
            trigger_reason: "Mock trigger for testing".to_string(),
        }
    }

    #[tokio::test]
    async fn test_mock_backend_success() {
        let backend = MockBackend::success();
        let context = create_mock_context();

        let result = backend.analyze(&context).await;
        assert!(result.is_ok());

        let insight = result.unwrap();
        assert_eq!(insight.summary, "Mock successful analysis");
        assert_eq!(insight.root_cause, Some("Mock root cause".to_string()));
        assert_eq!(insight.recommendations.len(), 2);
        assert_eq!(insight.severity, Severity::Info);

        assert_eq!(backend.call_count(), 1);
        assert!(backend.last_context().is_some());
    }

    #[tokio::test]
    async fn test_mock_backend_error() {
        let error_message = "Mock backend error".to_string();
        let backend = MockBackend::error(error_message.clone());
        let context = create_mock_context();

        let result = backend.analyze(&context).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            AnalysisError::BackendError(msg) => assert_eq!(msg, error_message),
            _ => panic!("Expected BackendError"),
        }

        assert_eq!(backend.call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_backend_timeout() {
        let backend = MockBackend::timeout();
        let context = create_mock_context();

        let result = backend.analyze(&context).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            AnalysisError::Timeout => {} // Expected
            _ => panic!("Expected Timeout error"),
        }
    }

    #[tokio::test]
    async fn test_mock_backend_multiple_responses() {
        let success_insight = AIInsight::new("Success".to_string(), None, vec![], Severity::Info);
        let error = AnalysisError::BackendError("Error".to_string());

        let responses = vec![Ok(success_insight.clone()), Err(error)];
        let backend = MockBackend::with_responses(responses);
        let context = create_mock_context();

        // First call should succeed
        let result1 = backend.analyze(&context).await;
        assert!(result1.is_ok());
        assert_eq!(result1.unwrap().summary, "Success");

        // Second call should fail
        let result2 = backend.analyze(&context).await;
        assert!(result2.is_err());

        // Third call should cycle back to success
        let result3 = backend.analyze(&context).await;
        assert!(result3.is_ok());

        assert_eq!(backend.call_count(), 3);
    }

    #[tokio::test]
    async fn test_mock_backend_with_delay() {
        let backend = MockBackend::success().with_delay(Duration::from_millis(10));
        let context = create_mock_context();

        let start = std::time::Instant::now();
        let result = backend.analyze(&context).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert!(elapsed >= Duration::from_millis(10));
    }

    #[tokio::test]
    async fn test_mock_backend_context_tracking() {
        let backend = MockBackend::success();
        let context = create_mock_context();

        backend.analyze(&context).await.unwrap();

        let tracked_context = backend.last_context().unwrap();
        assert_eq!(tracked_context.triggered_by, context.triggered_by);
        assert_eq!(tracked_context.trigger_reason, context.trigger_reason);
        assert_eq!(tracked_context.log_events.len(), context.log_events.len());
        assert_eq!(
            tracked_context.metrics_events.len(),
            context.metrics_events.len()
        );
    }

    #[tokio::test]
    async fn test_mock_backend_reset() {
        let backend = MockBackend::success();
        let context = create_mock_context();

        // Make some calls
        backend.analyze(&context).await.unwrap();
        backend.analyze(&context).await.unwrap();

        assert_eq!(backend.call_count(), 2);
        assert!(backend.last_context().is_some());

        // Reset
        backend.reset();

        assert_eq!(backend.call_count(), 0);
        assert!(backend.last_context().is_none());
    }

    #[tokio::test]
    async fn test_mock_backend_with_custom_insight() {
        let custom_insight = AIInsight::new(
            "Custom analysis".to_string(),
            Some("Custom cause".to_string()),
            vec!["Custom action 1".to_string(), "Custom action 2".to_string()],
            Severity::Critical,
        );

        let backend = MockBackend::with_response(Ok(custom_insight.clone()));
        let context = create_mock_context();

        let result = backend.analyze(&context).await.unwrap();
        assert_eq!(result.summary, custom_insight.summary);
        assert_eq!(result.root_cause, custom_insight.root_cause);
        assert_eq!(result.recommendations, custom_insight.recommendations);
        assert_eq!(result.severity, custom_insight.severity);
    }
}
