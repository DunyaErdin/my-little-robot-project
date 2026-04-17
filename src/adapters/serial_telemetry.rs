use log::{error, info, warn};

use crate::ports::telemetry::{TelemetryLevel, TelemetryPort};

pub struct SerialTelemetry;

impl SerialTelemetry {
    pub const fn new() -> Self {
        Self
    }
}

impl TelemetryPort for SerialTelemetry {
    fn log_event(&mut self, level: TelemetryLevel, component: &str, action: &str, detail: &str) {
        let message = format!("component={component} action={action} detail=\"{detail}\"");

        match level {
            TelemetryLevel::Info => info!("{message}"),
            TelemetryLevel::Warn => warn!("{message}"),
            TelemetryLevel::Error => error!("{message}"),
        }
    }
}
