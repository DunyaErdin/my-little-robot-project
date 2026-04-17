use crate::domain::{fault::FirmwareFault, robot_mode::RobotMode};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TelemetryLevel {
    Info,
    Warn,
    Error,
}

pub trait TelemetryPort {
    fn log_event(&mut self, level: TelemetryLevel, component: &str, action: &str, detail: &str);

    fn log_test_started(&mut self, test_name: &str) {
        self.log_event(TelemetryLevel::Info, test_name, "start", "test started");
    }

    fn log_test_succeeded(&mut self, test_name: &str, detail: &str) {
        self.log_event(TelemetryLevel::Info, test_name, "success", detail);
    }

    fn log_test_failed(&mut self, test_name: &str, fault: &FirmwareFault) {
        let detail = format!("kind={} message={}", fault.kind().as_str(), fault.message());
        self.log_event(TelemetryLevel::Error, test_name, "failure", &detail);
    }

    fn log_heartbeat(&mut self, mode: RobotMode, state: &str, emotion: &str) {
        let detail = format!("mode={} state={} emotion={}", mode.as_str(), state, emotion);
        self.log_event(TelemetryLevel::Info, "app", "heartbeat", &detail);
    }

    fn log_fault(&mut self, fault: &FirmwareFault) {
        let detail = format!("kind={} message={}", fault.kind().as_str(), fault.message());
        self.log_event(TelemetryLevel::Error, "fault", "active", &detail);
    }
}
