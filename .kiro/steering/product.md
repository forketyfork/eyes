# Product Overview

macOS System Observer is an AI-native monitoring tool that provides real-time insights into macOS runtime behavior through system logs and resource metrics.

## Core Purpose

Monitor macOS system health and proactively alert users to issues by:
- Streaming and analyzing macOS Unified Logs for errors and anomalies
- Tracking system resource consumption (CPU, memory, GPU, disk)
- Using AI to diagnose root causes and suggest fixes
- Delivering native macOS notifications for critical issues

## Key Features

- **Real-time Log Monitoring**: Streams macOS Unified Logs with intelligent filtering
- **Resource Tracking**: Monitors CPU, memory, GPU usage via `powermetrics`
- **AI Analysis**: Deep integration with local LLMs (Ollama) or cloud APIs (OpenAI)
- **Smart Alerting**: Rate-limited notifications to avoid alert fatigue
- **Privacy-First**: Designed to run locally with Ollama for sensitive system data

## Target Use Cases

- Detect apps consuming excessive memory or CPU
- Catch recurring errors in system logs before they become critical
- Get AI-powered diagnostics for system issues
- Monitor system health without enterprise-grade complexity
