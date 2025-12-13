# Changelog

## [Unreleased]

### Removed
- **Fallback monitoring functionality**: Removed `test_fallback_availability()` method and associated fallback monitoring logic from MetricsCollector
  - No longer attempts to use `vm_stat` and `top` commands when powermetrics is unavailable
  - Simplified architecture to focus on powermetrics as the primary metrics source
  - When powermetrics is unavailable, the system now enters degraded mode (log monitoring only)
  - Updated documentation across multiple files to reflect the simplified approach

### Changed
- **Degraded mode behavior**: When powermetrics is unavailable, the system now:
  - Continues log monitoring without metrics collection
  - Provides clear error messages about reduced functionality
  - Maintains system stability with limited capabilities
  - No longer attempts to generate synthetic metrics data

### Documentation Updates
- Updated `docs/subprocess-management.md` to remove fallback tool references
- Updated `docs/error-handling.md` to reflect degraded mode behavior
- Updated `docs/buffer-parsing.md` to remove fallback format documentation
- Updated `docs/collectors.md` to remove fallback system descriptions
- Updated `docs/metrics-collection.md` to remove fallback data source section
- Updated `docs/macos-integration.md` to remove fallback options
- Updated `docs/cli.md` to reflect new error messages

### Technical Details
- Removed `test_fallback_availability()` method from MetricsCollector
- Removed shell script generation for `vm_stat` memory pressure estimation
- Removed dual-format parsing (plist + JSON) - now only supports plist from powermetrics
- Simplified buffer parsing logic to focus on plist format only
- Reduced code complexity in MetricsCollector by removing fallback code paths
- Updated error messages to reflect degraded mode behavior