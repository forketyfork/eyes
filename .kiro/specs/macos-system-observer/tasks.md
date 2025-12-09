# Implementation Plan

- [x] 1. Initialize Rust project structure and dependencies
  - Create new Cargo project with workspace structure
  - Add all required dependencies to Cargo.toml (tokio, serde, chrono, reqwest, etc.)
  - Set up module structure (collectors, aggregator, triggers, ai, alerts, config)
  - Create basic error types using thiserror
  - _Requirements: 8.1, 8.2, 8.3_

- [ ] 2. Implement core data models
  - [x] 2.1 Define event structures and enums
    - Create LogEvent, MetricsEvent, MessageType, MemoryPressure, Severity types
    - Implement serialization/deserialization with serde
    - Add timestamp handling with chrono
    - _Requirements: 1.2, 2.2_

  - [x] 2.2 Write property test for log event parsing
    - **Property 1: Log parsing preserves structure**
    - **Validates: Requirements 1.2**

  - [ ] 2.3 Write property test for metrics event parsing
    - **Property 4: Metrics parsing extracts all fields**
    - **Validates: Requirements 2.2**

- [ ] 3. Implement configuration management
  - [ ] 3.1 Create Config struct and TOML parsing
    - Define Config struct with all configuration fields
    - Implement from_file method using toml crate
    - Implement default() method with safe defaults
    - Add validation for configuration values
    - _Requirements: 6.1, 6.5_

  - [ ] 3.2 Write property test for configuration loading
    - **Property 16: Configuration values are applied**
    - **Validates: Requirements 6.2, 6.3, 6.4**

  - [ ] 3.3 Write property test for invalid configuration handling
    - **Property 17: Invalid configuration uses safe defaults**
    - **Validates: Requirements 6.5**

- [ ] 4. Implement rolling buffer (Event Aggregator)
  - [ ] 4.1 Create EventAggregator with VecDeque storage
    - Implement add_log and add_metric methods
    - Implement get_recent_logs and get_recent_metrics with time filtering
    - Implement prune_old_entries for time-based expiration
    - Add capacity enforcement logic
    - _Requirements: 3.1, 3.2_

  - [ ] 4.2 Write property test for time-based expiration
    - **Property 6: Rolling buffer maintains time-based expiration**
    - **Validates: Requirements 3.1**

  - [ ] 4.3 Write property test for capacity limits
    - **Property 7: Rolling buffer enforces capacity limits**
    - **Validates: Requirements 3.2**

- [ ] 5. Implement Log Stream Collector
  - [ ] 5.1 Create LogCollector with subprocess management
    - Implement subprocess spawning for `log stream` command
    - Add JSON parsing for log entries
    - Implement channel-based event sending
    - Add error handling for malformed JSON
    - Implement automatic restart on subprocess failure
    - _Requirements: 1.1, 1.2, 1.4, 7.1_

  - [ ] 5.2 Write property test for malformed entry handling
    - **Property 2: Malformed entries don't halt processing**
    - **Validates: Requirements 1.4**

  - [ ] 5.3 Write property test for error/fault capture
    - **Property 3: Error and fault entries are captured**
    - **Validates: Requirements 1.3, 3.5**

  - [ ] 5.4 Write property test for subprocess restart
    - **Property 18: Log stream restart on failure**
    - **Validates: Requirements 7.1**

- [ ] 6. Implement Metrics Collector
  - [ ] 6.1 Create MetricsCollector with powermetrics subprocess
    - Implement subprocess spawning for `powermetrics` command
    - Add plist parsing for metrics output
    - Implement channel-based event sending
    - Add fallback to graceful degradation if powermetrics unavailable
    - _Requirements: 2.1, 2.2, 7.2_

  - [ ] 6.2 Write property test for memory pressure triggering
    - **Property 5: Memory pressure threshold triggers analysis**
    - **Validates: Requirements 2.5**

- [ ] 7. Implement Trigger Logic
  - [ ] 7.1 Create TriggerEngine and TriggerRule trait
    - Define TriggerRule trait with evaluate method
    - Implement TriggerEngine with rule collection
    - Create TriggerContext struct
    - _Requirements: 3.3, 3.4_

  - [ ] 7.2 Implement built-in trigger rules
    - Create ErrorFrequencyRule
    - Create MemoryPressureRule
    - Create CrashDetectionRule
    - Create ResourceSpikeRule
    - _Requirements: 3.3, 3.4_

  - [ ] 7.3 Write property test for trigger activation
    - **Property 8: Trigger activation on threshold breach**
    - **Validates: Requirements 3.3, 3.4**

- [ ] 8. Implement AI Analyzer
  - [ ] 8.1 Create LLMBackend trait and AIAnalyzer
    - Define LLMBackend trait with analyze method
    - Create AIInsight struct
    - Implement AIAnalyzer with backend delegation
    - Create prompt formatting logic
    - _Requirements: 4.1, 4.2_

  - [ ] 8.2 Write property test for prompt formatting
    - **Property 9: Prompt formatting includes context**
    - **Validates: Requirements 4.1**

  - [ ] 8.3 Write property test for backend invocation
    - **Property 10: AI backend receives analysis requests**
    - **Validates: Requirements 4.2**

  - [ ] 8.4 Implement OllamaBackend
    - Create HTTP client for Ollama API
    - Implement request formatting for Ollama
    - Parse Ollama JSON responses
    - Add error handling and timeouts
    - _Requirements: 4.3_

  - [ ] 8.5 Implement OpenAIBackend
    - Create HTTP client for OpenAI API
    - Add authentication header handling
    - Implement request formatting for OpenAI
    - Parse OpenAI JSON responses
    - Add error handling and timeouts
    - _Requirements: 4.4_

  - [ ] 8.6 Write property test for LLM response extraction
    - **Property 11: LLM response extraction**
    - **Validates: Requirements 4.5**

  - [ ] 8.7 Write property test for backend failure retry
    - **Property 19: AI backend failures are queued for retry**
    - **Validates: Requirements 7.3**

  - [ ] 8.8 Implement MockBackend for testing
    - Create mock backend with canned responses
    - Add configurable response delays
    - Add failure simulation capabilities
    - _Requirements: Testing infrastructure_

- [ ] 9. Implement Alert Manager
  - [ ] 9.1 Create AlertManager with rate limiting
    - Implement RateLimiter with time-based tracking
    - Create send_alert method with osascript execution
    - Add notification formatting logic
    - Implement error handling for notification failures
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [ ] 9.2 Write property test for critical issue notifications
    - **Property 12: Critical issues trigger notifications**
    - **Validates: Requirements 5.1**

  - [ ] 9.3 Write property test for notification content
    - **Property 13: Notification content completeness**
    - **Validates: Requirements 5.2, 5.3**

  - [ ] 9.4 Write property test for notification failure handling
    - **Property 14: Notification failures don't halt operation**
    - **Validates: Requirements 5.4**

  - [ ] 9.5 Write property test for rate limiting
    - **Property 15: Rate limiting prevents spam**
    - **Validates: Requirements 5.5**

- [ ] 10. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 11. Implement main application orchestration
  - [ ] 11.1 Create main application struct
    - Define SystemObserver struct with all components
    - Implement initialization logic
    - Set up mpsc channels for inter-thread communication
    - _Requirements: 1.1, 6.1_

  - [ ] 11.2 Implement thread spawning and coordination
    - Spawn log collector thread
    - Spawn metrics collector thread
    - Spawn analysis thread
    - Spawn notification thread
    - Add graceful shutdown handling
    - _Requirements: 1.5, 7.4_

  - [ ] 11.3 Wire event flow through the pipeline
    - Connect log collector to aggregator
    - Connect metrics collector to aggregator
    - Connect aggregator to trigger engine
    - Connect trigger engine to AI analyzer
    - Connect AI analyzer to alert manager
    - _Requirements: 1.1, 2.1, 3.1_

- [ ] 12. Implement CLI and entry point
  - [ ] 12.1 Create command-line argument parsing
    - Add clap dependency for CLI parsing
    - Define CLI arguments (--config, --verbose, --help)
    - Implement argument validation
    - _Requirements: 6.1_

  - [ ] 12.2 Implement main function
    - Load configuration
    - Initialize logging with env_logger
    - Create and start SystemObserver
    - Handle signals for graceful shutdown (SIGINT, SIGTERM)
    - _Requirements: 6.1, 7.5_

- [ ] 13. Add logging and observability
  - [ ] 13.1 Add structured logging throughout
    - Add log statements at key decision points
    - Log errors with context
    - Log configuration on startup
    - Log component lifecycle events
    - _Requirements: 7.2, 7.3, 7.4_

  - [ ] 13.2 Add metrics collection for self-monitoring
    - Track memory usage of the application
    - Track event processing rates
    - Track AI analysis latency
    - Track notification delivery success rate
    - _Requirements: 7.4_

- [ ] 14. Create example configuration file
  - [ ] 14.1 Write example config.toml
    - Include all configuration options with comments
    - Provide sensible defaults
    - Add examples for both Ollama and OpenAI backends
    - _Requirements: 6.1_

  - [ ] 14.2 Create configuration documentation
    - Document each configuration option
    - Provide examples for common use cases
    - Explain trigger rule customization
    - _Requirements: 6.1_

- [ ] 15. Final Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.
