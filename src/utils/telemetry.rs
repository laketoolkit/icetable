//! Telemetry collection (opt-in)

use serde::Serialize;

/// Telemetry event
#[derive(Debug, Serialize)]
pub struct TelemetryEvent {
    /// The type of event being recorded
    pub event_type: String,
    /// The UTC timestamp when the event occurred
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Additional properties associated with the event
    pub properties: std::collections::HashMap<String, String>,
}

/// Telemetry collector
pub struct TelemetryCollector {
    enabled: bool,
}

impl TelemetryCollector {
    /// Create a new telemetry collector
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Record an event
    pub fn record(&self, event: TelemetryEvent) {
        if !self.enabled {
            return;
        }

        // FUTURE: Send to telemetry backend when implemented
        // For now, events are logged at debug level for development
        log::debug!("Telemetry event: {:?}", event);
    }

    /// Record a command execution
    pub fn record_command(&self, command: &str, success: bool, duration_ms: u64) {
        let mut properties = std::collections::HashMap::new();
        properties.insert("command".to_string(), command.to_string());
        properties.insert("success".to_string(), success.to_string());
        properties.insert("duration_ms".to_string(), duration_ms.to_string());

        self.record(TelemetryEvent {
            event_type: "command_execution".to_string(),
            timestamp: chrono::Utc::now(),
            properties,
        });
    }
}
