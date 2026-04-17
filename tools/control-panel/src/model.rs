use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RobotMode {
    Booting,
    Idle,
    Diagnostics,
    Faulted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RobotEmotion {
    Neutral,
    Focused,
    Curious,
    Alert,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionCommand {
    Stop,
    Forward,
    Backward,
    TurnLeft,
    TurnRight,
    ArcadeDrive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSource {
    Safety,
    PanelButton,
    Slider,
    Gamepad,
    TestHarness,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestKind {
    FullHarness,
    Touch,
    Motor,
    Display,
    AudioIn,
    AudioOut,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemAction {
    ResetIdle,
    InjectFault,
    ClearFault,
    EmergencyStop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RobotSnapshot {
    pub overview: OverviewCard,
    pub motors: MotorStatus,
    pub sensors: SensorStatus,
    pub tests: TestStatus,
    pub gamepad: GamepadStatus,
    pub transport: TransportStatus,
    pub telemetry: Vec<TelemetryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewCard {
    pub harness_state: HarnessState,
    pub robot_mode: RobotMode,
    pub emotion: RobotEmotion,
    pub heartbeat_count: u64,
    pub fault: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotorStatus {
    pub command: MotionCommand,
    pub source: ControlSource,
    pub throttle: f32,
    pub turn: f32,
    pub left_output: f32,
    pub right_output: f32,
    pub gpio: MotorGpioState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotorGpioState {
    pub in1: bool,
    pub in2: bool,
    pub in3: bool,
    pub in4: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorStatus {
    pub pet_touch: bool,
    pub record_touch: bool,
    pub display_ready: bool,
    pub display_message: String,
    pub audio_in_placeholder_ready: bool,
    pub audio_out_placeholder_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestStatus {
    pub running: Option<TestKind>,
    pub last_completed: Option<TestKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamepadStatus {
    pub connected: bool,
    pub driving_enabled: bool,
    pub id: Option<String>,
    pub axes: Vec<f32>,
    pub buttons: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportStatus {
    pub backend: String,
    pub target: String,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct SensorOverrideRequest {
    pub pet_touch: Option<bool>,
    pub record_touch: Option<bool>,
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
