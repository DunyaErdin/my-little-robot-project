use serde::{Deserialize, Serialize};

use crate::{
    app::state::HarnessState as AppHarnessState,
    domain::{emotion::RobotEmotion as DomainEmotion, robot_mode::RobotMode as DomainRobotMode},
};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessState {
    Boot,
    Idle,
    TestTouch,
    TestMotor,
    TestDisplay,
    TestAudioInPlaceholder,
    TestAudioOutPlaceholder,
    Fault,
}

impl From<AppHarnessState> for HarnessState {
    fn from(value: AppHarnessState) -> Self {
        match value {
            AppHarnessState::Boot => Self::Boot,
            AppHarnessState::Idle => Self::Idle,
            AppHarnessState::TestTouch => Self::TestTouch,
            AppHarnessState::TestMotor => Self::TestMotor,
            AppHarnessState::TestDisplay => Self::TestDisplay,
            AppHarnessState::TestAudioInPlaceholder => Self::TestAudioInPlaceholder,
            AppHarnessState::TestAudioOutPlaceholder => Self::TestAudioOutPlaceholder,
            AppHarnessState::Fault => Self::Fault,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RobotMode {
    Booting,
    Idle,
    Diagnostics,
    Faulted,
}

impl From<DomainRobotMode> for RobotMode {
    fn from(value: DomainRobotMode) -> Self {
        match value {
            DomainRobotMode::Booting => Self::Booting,
            DomainRobotMode::Idle => Self::Idle,
            DomainRobotMode::Diagnostics => Self::Diagnostics,
            DomainRobotMode::Faulted => Self::Faulted,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RobotEmotion {
    Neutral,
    Focused,
    Curious,
    Alert,
}

impl From<DomainEmotion> for RobotEmotion {
    fn from(value: DomainEmotion) -> Self {
        match value {
            DomainEmotion::Neutral => Self::Neutral,
            DomainEmotion::Focused => Self::Focused,
            DomainEmotion::Curious => Self::Curious,
            DomainEmotion::Alert => Self::Alert,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MotionCommand {
    Stop,
    Forward,
    Backward,
    TurnLeft,
    TurnRight,
    ArcadeDrive,
}

impl MotionCommand {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Forward => "forward",
            Self::Backward => "backward",
            Self::TurnLeft => "turn_left",
            Self::TurnRight => "turn_right",
            Self::ArcadeDrive => "arcade_drive",
        }
    }

    pub const fn label_tr(self) -> &'static str {
        match self {
            Self::Stop => "Dur",
            Self::Forward => "İleri",
            Self::Backward => "Geri",
            Self::TurnLeft => "Sola Dön",
            Self::TurnRight => "Sağa Dön",
            Self::ArcadeDrive => "Arcade Sürüş",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSource {
    Safety,
    PanelButton,
    Slider,
    Gamepad,
    TestHarness,
}

impl ControlSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Safety => "safety",
            Self::PanelButton => "panel_button",
            Self::Slider => "slider",
            Self::Gamepad => "gamepad",
            Self::TestHarness => "test_harness",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TestKind {
    FullHarness,
    Touch,
    Motor,
    Display,
    AudioIn,
    AudioOut,
}

impl TestKind {
    pub const fn label_tr(self) -> &'static str {
        match self {
            Self::FullHarness => "Tam Test Harnesi",
            Self::Touch => "Dokunmatik Testi",
            Self::Motor => "Motor Testi",
            Self::Display => "Ekran Testi",
            Self::AudioIn => "Mikrofon Hazırlık Testi",
            Self::AudioOut => "Hoparlör Hazırlık Testi",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemAction {
    ResetIdle,
    EmergencyStop,
}

impl SystemAction {
    pub const fn label_tr(self) -> &'static str {
        match self {
            Self::ResetIdle => "Boşa Al / Dur",
            Self::EmergencyStop => "Acil Durdur",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TestExecutionState {
    Idle,
    Running,
    Passed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PinConnectionStatus {
    Present,
    Absent,
    ManualConfirmed,
    ManualRequired,
    Unknown,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PinVerificationMethod {
    AutomaticProbe,
    LiveSignal,
    ManualAck,
}

#[derive(Debug, Clone, Serialize)]
pub struct RobotSnapshot {
    pub overview: OverviewCard,
    pub controls: ControlAccess,
    pub motors: MotorStatus,
    pub sensors: SensorStatus,
    pub inspection: InspectionStatus,
    pub tests: TestStatus,
    pub gamepad: GamepadStatus,
    pub transport: TransportStatus,
    pub telemetry: Vec<TelemetryEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverviewCard {
    pub harness_state: HarnessState,
    pub robot_mode: RobotMode,
    pub emotion: RobotEmotion,
    pub heartbeat_count: u64,
    pub fault: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlAccess {
    pub motion_run_allowed: bool,
    pub motion_blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MotorStatus {
    pub command: MotionCommand,
    pub source: ControlSource,
    pub throttle: f32,
    pub turn: f32,
    pub left_output: f32,
    pub right_output: f32,
    pub gpio: MotorGpioState,
}

#[derive(Debug, Clone, Serialize)]
pub struct MotorGpioState {
    pub in1: bool,
    pub in2: bool,
    pub in3: bool,
    pub in4: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SensorStatus {
    pub pet_touch: bool,
    pub record_touch: bool,
    pub pet_touch_seen: bool,
    pub record_touch_seen: bool,
    pub display_ready: bool,
    pub display_message: String,
    pub audio_in_placeholder_ready: bool,
    pub audio_out_placeholder_ready: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectionStatus {
    pub scan_completed: bool,
    pub scan_in_progress: bool,
    pub post_passed: bool,
    pub last_scan_at: String,
    pub last_scan_summary: String,
    pub manual_checks: ManualChecks,
    pub limitations: Vec<String>,
    pub pins: Vec<PinStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualChecks {
    pub motor_driver_wired: bool,
    pub microphone_wired: bool,
    pub speaker_wired: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PinStatus {
    pub key: String,
    pub label: String,
    pub gpio: u8,
    pub role: String,
    pub direction: String,
    pub signal: String,
    pub connection_status: PinConnectionStatus,
    pub verification_method: PinVerificationMethod,
    pub detail: String,
    pub required_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestStatus {
    pub running: Option<TestKind>,
    pub last_completed: Option<TestKind>,
    pub reports: Vec<TestReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestReport {
    pub test: TestKind,
    pub state: TestExecutionState,
    pub run_allowed: bool,
    pub blocked_reason: Option<String>,
    pub last_detail: String,
    pub last_updated: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GamepadStatus {
    pub connected: bool,
    pub driving_enabled: bool,
    pub id: Option<String>,
    pub axes: Vec<f32>,
    pub buttons: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransportStatus {
    pub backend: String,
    pub target: String,
    pub connected: bool,
    pub ssid: String,
    pub password_hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryEntry {
    pub timestamp: String,
    pub level: String,
    pub component: String,
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MotorActionRequest {
    pub action: MotionCommand,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArcadeDriveRequest {
    pub throttle: f32,
    pub turn: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunTestRequest {
    pub test: TestKind,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GamepadUpdateRequest {
    pub connected: bool,
    pub driving_enabled: bool,
    pub id: Option<String>,
    pub axes: Vec<f32>,
    pub buttons: Vec<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemActionRequest {
    pub action: SystemAction,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManualChecksRequest {
    pub motor_driver_wired: bool,
    pub microphone_wired: bool,
    pub speaker_wired: bool,
}
