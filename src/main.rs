use clap::Parser;
use eyes::aggregator::EventAggregator;
use eyes::ai::{AIAnalyzer, MockBackend, OllamaBackend, OpenAIBackend};
use eyes::alerts::AlertManager;
use eyes::collectors::{LogCollector, MetricsCollector};
use eyes::config::{AIBackendConfig, Config};
use eyes::error::ConfigError;
use eyes::events::{LogEvent, MetricsEvent, Severity};
use eyes::triggers::{
    CrashDetectionRule, ErrorFrequencyRule, MemoryPressureRule, ResourceSpikeRule, TriggerEngine,
};
use log::{error, info, warn};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Command-line arguments for the macOS System Observer
#[derive(Parser)]
#[command(
    name = "eyes",
    about = "macOS System Observer - AI-powered system monitoring and alerting",
    long_about = "A Rust-based application that monitors macOS system logs and resource consumption, \
                  using AI to analyze patterns, detect anomalies, and provide actionable insights \
                  through native notifications."
)]
struct Cli {
    /// Path to configuration file
    #[arg(
        short,
        long,
        value_name = "FILE",
        help = "Configuration file path (TOML format)"
    )]
    config: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(
        short,
        long,
        help = "Enable verbose logging output (sets RUST_LOG=debug)"
    )]
    verbose: bool,
}

impl Cli {
    /// Validate the CLI arguments
    ///
    /// # Returns
    ///
    /// `Ok(())` if all arguments are valid, `Err(String)` with error message otherwise
    fn validate(&self) -> Result<(), String> {
        // Validate config file path if provided
        if let Some(ref config_path) = self.config {
            // Only validate that it's not a directory if it exists
            // Missing files will be handled gracefully by falling back to defaults
            if config_path.exists() {
                if !config_path.is_file() {
                    return Err(format!(
                        "Configuration path is not a file: {}",
                        config_path.display()
                    ));
                }

                // Check if file has .toml extension (optional but recommended)
                if let Some(extension) = config_path.extension() {
                    if extension != "toml" {
                        warn!(
                            "Configuration file does not have .toml extension: {}",
                            config_path.display()
                        );
                    }
                }
            }
            // Note: Missing files are handled gracefully by SystemObserver::load_config
            // which will warn and fall back to defaults
        }

        Ok(())
    }

    /// Convert config path to string safely, handling non-UTF-8 paths
    ///
    /// # Returns
    ///
    /// `Ok(Some(path_str))` if config is provided and valid UTF-8,
    /// `Ok(None)` if no config provided,
    /// `Err(String)` if config path contains invalid UTF-8
    fn config_path_str(&self) -> Result<Option<&str>, String> {
        match &self.config {
            Some(path) => match path.to_str() {
                Some(path_str) => Ok(Some(path_str)),
                None => Err(format!(
                    "Configuration file path contains invalid UTF-8 characters: {}",
                    path.display()
                )),
            },
            None => Ok(None),
        }
    }
}

/// Messages sent to the analysis thread
#[derive(Debug)]
enum AnalysisMessage {
    LogEvent(LogEvent),
    MetricsEvent(MetricsEvent),
    Shutdown,
}

/// Main application struct that orchestrates all system observer components
///
/// SystemObserver coordinates the data flow between collectors, aggregator,
/// trigger engine, AI analyzer, and alert manager. It manages the lifecycle
/// of all components and handles graceful shutdown.
pub struct SystemObserver {
    /// Application configuration
    config: Config,

    /// Log collector for streaming macOS Unified Logs
    log_collector: LogCollector,

    /// Metrics collector for system resource monitoring
    metrics_collector: MetricsCollector,

    /// Event aggregator with rolling buffer
    event_aggregator: Arc<Mutex<EventAggregator>>,

    /// Trigger engine for determining when to invoke AI analysis
    #[allow(dead_code)] // Used in thread creation but moved, so appears unused
    trigger_engine: TriggerEngine,

    /// AI analyzer for generating insights
    #[allow(dead_code)] // Used in thread creation but moved, so appears unused
    ai_analyzer: AIAnalyzer,

    /// Alert manager for delivering notifications
    alert_manager: Arc<Mutex<AlertManager>>,

    /// Channel for log events from collector to aggregator
    #[allow(dead_code)] // Used by collectors but appears unused
    log_sender: Sender<LogEvent>,
    log_receiver: Receiver<LogEvent>,

    /// Channel for metrics events from collector to aggregator
    #[allow(dead_code)] // Used by collectors but appears unused
    metrics_sender: Sender<MetricsEvent>,
    metrics_receiver: Receiver<MetricsEvent>,

    /// Shutdown signal
    shutdown_sender: Sender<()>,
    shutdown_receiver: Receiver<()>,

    /// Additional shutdown senders for threads
    shutdown_senders: Vec<Sender<()>>,

    /// Thread handles for cleanup
    thread_handles: Vec<JoinHandle<()>>,

    /// Channel for sending messages to analysis thread
    analysis_sender: Option<Sender<AnalysisMessage>>,
}

impl SystemObserver {
    /// Create a new SystemObserver with the given configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Application configuration
    ///
    /// # Returns
    ///
    /// A new SystemObserver instance ready to start monitoring
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if the configuration is invalid or if component
    /// initialization fails.
    pub fn new(config: Config) -> Result<Self, ConfigError> {
        info!("Initializing SystemObserver with configuration");

        // Create communication channels
        let (log_sender, log_receiver) = mpsc::channel();
        let (metrics_sender, metrics_receiver) = mpsc::channel();
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();

        // Initialize event aggregator
        let event_aggregator = Arc::new(Mutex::new(EventAggregator::new(
            chrono::Duration::seconds(config.buffer.max_age_seconds as i64),
            config.buffer.max_size,
        )));

        // Initialize collectors
        let log_collector = LogCollector::new(config.logging.predicate.clone(), log_sender.clone());
        let metrics_collector = MetricsCollector::new(
            Duration::from_secs(config.metrics.interval_seconds),
            metrics_sender.clone(),
        );

        // Initialize trigger engine with built-in rules
        let mut trigger_engine = TriggerEngine::new();
        trigger_engine.add_rule(Box::new(ErrorFrequencyRule::new(
            config.triggers.error_threshold,
            config.triggers.error_window_seconds as i64,
            Severity::Warning,
        )));
        trigger_engine.add_rule(Box::new(MemoryPressureRule::new(
            config.triggers.memory_threshold,
            Severity::Warning,
        )));
        trigger_engine.add_rule(Box::new(CrashDetectionRule::new(
            vec![
                "crash".to_string(),
                "abort".to_string(),
                "segfault".to_string(),
            ],
            Severity::Critical,
        )));
        trigger_engine.add_rule(Box::new(ResourceSpikeRule::new(
            1000.0, // CPU threshold in milliwatts
            2000.0, // GPU threshold in milliwatts
            30,     // window seconds
            Severity::Warning,
        )));

        // Initialize AI analyzer with configured backend
        let ai_analyzer = match &config.ai.backend {
            AIBackendConfig::Ollama { endpoint, model } => {
                let backend = OllamaBackend::new(endpoint.clone(), model.clone());
                AIAnalyzer::with_backend(Arc::new(backend))
            }
            AIBackendConfig::OpenAI { api_key, model } => {
                let backend = OpenAIBackend::new(api_key.clone(), model.clone());
                AIAnalyzer::with_backend(Arc::new(backend))
            }
            AIBackendConfig::Mock => {
                let backend = MockBackend::success();
                AIAnalyzer::with_backend(Arc::new(backend))
            }
        };

        // Initialize alert manager
        let alert_manager = Arc::new(Mutex::new(AlertManager::new(
            config.alerts.rate_limit_per_minute,
        )));

        Ok(SystemObserver {
            config,
            log_collector,
            metrics_collector,
            event_aggregator,
            trigger_engine,
            ai_analyzer,
            alert_manager,
            log_sender,
            log_receiver,
            metrics_sender,
            metrics_receiver,
            shutdown_sender,
            shutdown_receiver,
            shutdown_senders: Vec::new(),
            thread_handles: Vec::new(),
            analysis_sender: None,
        })
    }

    /// Load configuration from file or use defaults
    ///
    /// # Arguments
    ///
    /// * `config_path` - Optional path to configuration file
    ///
    /// # Returns
    ///
    /// Loaded configuration or default configuration if file not found or invalid
    pub fn load_config(config_path: Option<&str>) -> Result<Config, ConfigError> {
        match config_path {
            Some(path) => {
                info!("Loading configuration from: {}", path);
                match Config::from_file(std::path::Path::new(path)) {
                    Ok(config) => Ok(config),
                    Err(ConfigError::ReadError(_)) => {
                        warn!(
                            "Configuration file '{}' not found or unreadable, using defaults",
                            path
                        );
                        Ok(Config::default())
                    }
                    Err(e) => {
                        // Report errors and use safe default values for invalid configuration
                        error!("Configuration error in '{}': {}", path, e);
                        warn!("Using default configuration due to invalid config file");
                        Ok(Config::default())
                    }
                }
            }
            None => {
                info!("Using default configuration");
                Ok(Config::default())
            }
        }
    }

    /// Start the system observer and all its components
    ///
    /// This method spawns all necessary threads and begins monitoring.
    /// It returns immediately after starting all threads.
    ///
    /// # Errors
    ///
    /// Returns `CollectorError` if any collector fails to start.
    pub fn start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting SystemObserver components");

        // Spawn analysis thread first (creates the analysis_sender)
        let analysis_thread = self.spawn_analysis_thread()?;
        self.thread_handles.push(analysis_thread);

        // Spawn event forwarding threads
        let log_forwarding_thread = self.spawn_log_forwarding_thread()?;
        self.thread_handles.push(log_forwarding_thread);

        let metrics_forwarding_thread = self.spawn_metrics_forwarding_thread()?;
        self.thread_handles.push(metrics_forwarding_thread);

        // Spawn notification thread
        let notification_thread = self.spawn_notification_thread()?;
        self.thread_handles.push(notification_thread);

        // Start collectors last
        self.log_collector.start()?;
        info!("Log collector started");

        self.metrics_collector.start()?;
        info!("Metrics collector started");

        info!("All SystemObserver components started successfully");
        Ok(())
    }

    /// Stop the system observer and all its components
    ///
    /// This method gracefully shuts down all threads and collectors.
    pub fn stop(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Stopping SystemObserver components");

        // Send shutdown signal to analysis thread
        if let Some(ref sender) = self.analysis_sender {
            if let Err(e) = sender.send(AnalysisMessage::Shutdown) {
                error!("Failed to send analysis shutdown signal: {}", e);
            }
        }

        // Send shutdown signals to all other threads
        for sender in &self.shutdown_senders {
            if let Err(e) = sender.send(()) {
                error!("Failed to send shutdown signal to thread: {}", e);
            }
        }

        // Stop collectors
        if let Err(e) = self.log_collector.stop() {
            error!("Failed to stop log collector: {}", e);
        }

        if let Err(e) = self.metrics_collector.stop() {
            error!("Failed to stop metrics collector: {}", e);
        }

        // Wait for threads to finish
        for handle in self.thread_handles.drain(..) {
            if let Err(e) = handle.join() {
                error!("Thread failed to join: {:?}", e);
            }
        }

        info!("SystemObserver stopped successfully");
        Ok(())
    }

    /// Wait for shutdown signal (blocking)
    ///
    /// This method blocks until a shutdown signal is received or an error occurs.
    pub fn wait_for_shutdown(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Waiting for shutdown signal...");

        // Set up signal handling for graceful shutdown
        let shutdown_receiver = &self.shutdown_receiver;

        // Block until shutdown signal received
        match shutdown_receiver.recv() {
            Ok(()) => {
                info!("Shutdown signal received");
                Ok(())
            }
            Err(e) => {
                error!("Error waiting for shutdown: {}", e);
                Err(Box::new(e))
            }
        }
    }

    /// Spawn the analysis thread that processes events and triggers AI analysis
    fn spawn_analysis_thread(
        &mut self,
    ) -> Result<JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
        // Move components into the thread
        let event_aggregator = Arc::clone(&self.event_aggregator);
        let alert_manager = Arc::clone(&self.alert_manager);

        // Create a new trigger engine for the thread (since it doesn't implement Clone)
        let mut trigger_engine = TriggerEngine::new();
        trigger_engine.add_rule(Box::new(ErrorFrequencyRule::new(
            self.config.triggers.error_threshold,
            self.config.triggers.error_window_seconds as i64,
            Severity::Warning,
        )));
        trigger_engine.add_rule(Box::new(MemoryPressureRule::new(
            self.config.triggers.memory_threshold,
            Severity::Warning,
        )));
        trigger_engine.add_rule(Box::new(CrashDetectionRule::new(
            vec![
                "crash".to_string(),
                "abort".to_string(),
                "segfault".to_string(),
            ],
            Severity::Critical,
        )));
        trigger_engine.add_rule(Box::new(ResourceSpikeRule::new(
            1000.0, // CPU threshold in milliwatts
            2000.0, // GPU threshold in milliwatts
            30,     // window seconds
            Severity::Warning,
        )));

        // Create a new AI analyzer for the thread
        let ai_analyzer = match &self.config.ai.backend {
            AIBackendConfig::Ollama { endpoint, model } => {
                let backend = OllamaBackend::new(endpoint.clone(), model.clone());
                AIAnalyzer::with_backend(Arc::new(backend))
            }
            AIBackendConfig::OpenAI { api_key, model } => {
                let backend = OpenAIBackend::new(api_key.clone(), model.clone());
                AIAnalyzer::with_backend(Arc::new(backend))
            }
            AIBackendConfig::Mock => {
                let backend = MockBackend::success();
                AIAnalyzer::with_backend(Arc::new(backend))
            }
        };

        // Create channels for communication with the analysis thread
        let (analysis_sender, analysis_receiver) = mpsc::channel::<AnalysisMessage>();

        // Store the sender for later use
        self.analysis_sender = Some(analysis_sender);

        let handle = std::thread::spawn(move || {
            info!("Analysis thread started");

            loop {
                match analysis_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(AnalysisMessage::LogEvent(log_event)) => {
                        if let Ok(mut aggregator) = event_aggregator.lock() {
                            aggregator.add_log(log_event);
                            aggregator.prune_old_entries();
                        }
                    }
                    Ok(AnalysisMessage::MetricsEvent(metrics_event)) => {
                        if let Ok(mut aggregator) = event_aggregator.lock() {
                            aggregator.add_metric(metrics_event);
                            aggregator.prune_old_entries();
                        }
                    }
                    Ok(AnalysisMessage::Shutdown) => {
                        info!("Analysis thread received shutdown signal");
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // Timeout is expected, continue processing
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        info!("Analysis thread channel disconnected");
                        break;
                    }
                }

                // Check triggers and run AI analysis if needed
                if let Ok(aggregator) = event_aggregator.lock() {
                    let recent_logs_refs = aggregator.get_recent_logs(chrono::Duration::minutes(5));
                    let recent_metrics_refs =
                        aggregator.get_recent_metrics(chrono::Duration::minutes(5));

                    // Convert references to owned values
                    let recent_logs: Vec<LogEvent> =
                        recent_logs_refs.into_iter().cloned().collect();
                    let recent_metrics: Vec<MetricsEvent> =
                        recent_metrics_refs.into_iter().cloned().collect();

                    let contexts = trigger_engine.evaluate(&recent_logs, &recent_metrics);

                    for context in contexts {
                        info!("Trigger activated: {}", context.triggered_by);

                        // Create a simple runtime for the async call
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        match rt.block_on(ai_analyzer.analyze(&context)) {
                            Ok(insight) => {
                                info!("AI analysis completed: {}", insight.summary);

                                if let Ok(mut alert_mgr) = alert_manager.lock() {
                                    if let Err(e) = alert_mgr.send_alert(&insight) {
                                        error!("Failed to send alert: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("AI analysis failed: {}", e);
                            }
                        }
                    }
                }
            }

            info!("Analysis thread stopped");
        });

        Ok(handle)
    }

    /// Spawn thread that forwards log events to analysis thread
    fn spawn_log_forwarding_thread(
        &mut self,
    ) -> Result<JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
        // Create a dedicated shutdown channel for this thread
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();
        self.shutdown_senders.push(shutdown_sender);

        let analysis_sender = self
            .analysis_sender
            .as_ref()
            .ok_or("Analysis sender not initialized")?
            .clone();

        // We need to move the log_receiver into the thread
        // This means we can't use it elsewhere, which is fine for our architecture
        let log_receiver = std::mem::replace(&mut self.log_receiver, {
            let (_, dummy_receiver) = mpsc::channel();
            dummy_receiver
        });

        let handle = std::thread::spawn(move || {
            info!("Log forwarding thread started");

            loop {
                // Check for shutdown signal (non-blocking)
                if shutdown_receiver.try_recv().is_ok() {
                    info!("Log forwarding thread received shutdown signal");
                    break;
                }

                // Forward log events to analysis thread
                match log_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(log_event) => {
                        if let Err(e) = analysis_sender.send(AnalysisMessage::LogEvent(log_event)) {
                            error!("Failed to forward log event to analysis thread: {}", e);
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // Timeout is expected, continue
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        info!("Log receiver disconnected");
                        break;
                    }
                }
            }

            info!("Log forwarding thread stopped");
        });

        Ok(handle)
    }

    /// Spawn thread that forwards metrics events to analysis thread
    fn spawn_metrics_forwarding_thread(
        &mut self,
    ) -> Result<JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
        // Create a dedicated shutdown channel for this thread
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();
        self.shutdown_senders.push(shutdown_sender);

        let analysis_sender = self
            .analysis_sender
            .as_ref()
            .ok_or("Analysis sender not initialized")?
            .clone();

        // Move the metrics_receiver into the thread
        let metrics_receiver = std::mem::replace(&mut self.metrics_receiver, {
            let (_, dummy_receiver) = mpsc::channel();
            dummy_receiver
        });

        let handle = std::thread::spawn(move || {
            info!("Metrics forwarding thread started");

            loop {
                // Check for shutdown signal (non-blocking)
                if shutdown_receiver.try_recv().is_ok() {
                    info!("Metrics forwarding thread received shutdown signal");
                    break;
                }

                // Forward metrics events to analysis thread
                match metrics_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(metrics_event) => {
                        if let Err(e) =
                            analysis_sender.send(AnalysisMessage::MetricsEvent(metrics_event))
                        {
                            error!("Failed to forward metrics event to analysis thread: {}", e);
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // Timeout is expected, continue
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        info!("Metrics receiver disconnected");
                        break;
                    }
                }
            }

            info!("Metrics forwarding thread stopped");
        });

        Ok(handle)
    }

    /// Spawn the notification thread that processes queued alerts
    fn spawn_notification_thread(
        &mut self,
    ) -> Result<JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
        // Create a dedicated shutdown channel for this thread
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();
        self.shutdown_senders.push(shutdown_sender);

        let alert_manager = Arc::clone(&self.alert_manager);

        let handle = std::thread::spawn(move || {
            info!("Notification thread started");

            loop {
                // Check for shutdown signal (non-blocking)
                if shutdown_receiver.try_recv().is_ok() {
                    info!("Notification thread received shutdown signal");
                    break;
                }

                // Process queued alerts
                if let Ok(mut alert_mgr) = alert_manager.lock() {
                    let _ = alert_mgr.tick();
                }

                // Sleep briefly to avoid busy waiting
                std::thread::sleep(Duration::from_millis(500));
            }

            info!("Notification thread stopped");
        });

        Ok(handle)
    }
}

fn main() {
    // Parse command-line arguments
    let cli = Cli::parse();

    // Initialize logging based on verbosity
    if cli.verbose {
        std::env::set_var("RUST_LOG", "debug");
    }
    env_logger::init();

    info!("Starting macOS System Observer");

    // Validate CLI arguments
    if let Err(e) = cli.validate() {
        error!("Invalid arguments: {}", e);
        std::process::exit(1);
    }

    // Load configuration with safe path handling
    let config_path = match cli.config_path_str() {
        Ok(path) => path,
        Err(e) => {
            error!("Invalid configuration path: {}", e);
            std::process::exit(1);
        }
    };

    let config = match SystemObserver::load_config(config_path) {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    // Create system observer
    let mut observer = match SystemObserver::new(config) {
        Ok(observer) => observer,
        Err(e) => {
            error!("Failed to initialize SystemObserver: {}", e);
            std::process::exit(1);
        }
    };

    info!("SystemObserver initialized successfully");

    // Start the observer
    if let Err(e) = observer.start() {
        error!("Failed to start SystemObserver: {}", e);
        std::process::exit(1);
    }

    // Set up signal handling for graceful shutdown (SIGINT, SIGTERM)
    let shutdown_sender = observer.shutdown_sender.clone();
    ctrlc::set_handler(move || {
        info!("Received interrupt signal (SIGINT), shutting down gracefully...");
        if let Err(e) = shutdown_sender.send(()) {
            error!("Failed to send shutdown signal: {}", e);
        }
    })
    .expect("Error setting SIGINT handler for graceful shutdown");

    // Note: ctrlc crate primarily handles SIGINT (Ctrl+C)
    // For production deployment, consider using signal-hook crate for comprehensive
    // SIGTERM handling, especially when running as a daemon or service

    info!("System Observer is running. Press Ctrl+C to stop.");

    // Wait for shutdown
    if let Err(e) = observer.wait_for_shutdown() {
        error!("Error during shutdown wait: {}", e);
    }

    // Stop the observer
    if let Err(e) = observer.stop() {
        error!("Error during shutdown: {}", e);
        std::process::exit(1);
    }

    info!("SystemObserver shutdown complete");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_validation_with_existing_file() {
        // Create a temporary file for testing
        let temp_file = std::env::temp_dir().join("test_config.toml");
        std::fs::write(&temp_file, "[ai]\nbackend = \"mock\"").unwrap();

        let cli = Cli {
            config: Some(temp_file.clone()),
            verbose: false,
        };

        assert!(cli.validate().is_ok());

        // Clean up
        std::fs::remove_file(&temp_file).unwrap();
    }

    #[test]
    fn test_cli_validation_with_missing_file() {
        let cli = Cli {
            config: Some(PathBuf::from("/nonexistent/config.toml")),
            verbose: false,
        };

        // Should not fail - missing files are handled gracefully
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn test_cli_validation_with_directory() {
        let cli = Cli {
            config: Some(PathBuf::from("/tmp")),
            verbose: false,
        };

        // Should fail - directories are not valid config files
        assert!(cli.validate().is_err());
    }

    #[test]
    fn test_cli_validation_no_config() {
        let cli = Cli {
            config: None,
            verbose: false,
        };

        assert!(cli.validate().is_ok());
    }

    #[test]
    fn test_config_path_str_with_valid_path() {
        let cli = Cli {
            config: Some(PathBuf::from("config.toml")),
            verbose: false,
        };

        let result = cli.config_path_str().unwrap();
        assert_eq!(result, Some("config.toml"));
    }

    #[test]
    fn test_config_path_str_no_config() {
        let cli = Cli {
            config: None,
            verbose: false,
        };

        let result = cli.config_path_str().unwrap();
        assert_eq!(result, None);
    }
}
