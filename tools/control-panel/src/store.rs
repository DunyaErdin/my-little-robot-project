use std::{collections::VecDeque, sync::Arc};

use chrono::Local;
use tokio::sync::Mutex;

use crate::model::{
    ControlSource, GamepadStatus, HarnessState, MotionCommand, MotorGpioState, MotorStatus,
    OverviewCard, RobotEmotion, RobotMode, RobotSnapshot, SensorStatus, TelemetryEntry, TestStatus,
    TransportStatus,
};

const TELEMETRY_LIMIT: usize = 200;

#[derive(Clone)]
pub struct PanelStore {
    inner: Arc<Mutex<PanelRuntime>>,
}

pub struct PanelRuntime {
    pub(crate) snapshot: RobotSnapshot,
    telemetry_ring: VecDeque<TelemetryEntry>,
}

impl PanelStore {
    pub fn new() -> Self {
        let mut runtime = PanelRuntime {
            snapshot: RobotSnapshot {
                overview: OverviewCard {
                    harness_state: HarnessState::Boot,
                    robot_mode: RobotMode::Booting,
                    emotion: RobotEmotion::Focused,
                    heartbeat_count: 0,
                    fault: None,
                    updated_at: timestamp_now(),
                },
                motors: MotorStatus {
                    command: MotionCommand::Stop,
                    source: ControlSource::Safety,
                    throttle: 0.0,
                    turn: 0.0,
                    left_output: 0.0,
                    right_output: 0.0,
                    gpio: MotorGpioState {
                        in1: false,
                        in2: false,
                        in3: false,
                        in4: false,
                    },
                },
                sensors: SensorStatus {
                    pet_touch: false,
                    record_touch: false,
                    display_ready: true,
                    display_message: "BOOT: localhost control panel ready".to_string(),
                    audio_in_placeholder_ready: true,
                    audio_out_placeholder_ready: true,
                },
                tests: TestStatus {
                    running: None,
                    last_completed: None,
                },
                gamepad: GamepadStatus {
                    connected: false,
                    driving_enabled: false,
                    id: None,
                    axes: vec![],
                    buttons: vec![],
                },
                transport: TransportStatus {
                    backend: "mock_robot_transport".to_string(),
                    target: "localhost panel state".to_string(),
                    connected: true,
                },
                telemetry: Vec::new(),
            },
            telemetry_ring: VecDeque::with_capacity(TELEMETRY_LIMIT),
        };

        runtime.transition(HarnessState::Idle);
        runtime.log(
            "info",
            "panel",
            "boot_completed",
            "localhost control panel initialized with mock robot backend",
        );

        Self {
            inner: Arc::new(Mutex::new(runtime)),
        }
    }

    pub async fn snapshot(&self) -> RobotSnapshot {
        let runtime = self.inner.lock().await;
        runtime.snapshot.clone()
    }

    pub async fn with_mut<R>(&self, f: impl FnOnce(&mut PanelRuntime) -> R) -> R {
        let mut runtime = self.inner.lock().await;
        f(&mut runtime)
    }
}

impl PanelRuntime {
    pub fn log(&mut self, level: &str, component: &str, action: &str, detail: &str) {
        let entry = TelemetryEntry {
            timestamp: timestamp_now(),
            level: level.to_string(),
            component: component.to_string(),
            action: action.to_string(),
            detail: detail.to_string(),
        };

        if self.telemetry_ring.len() == TELEMETRY_LIMIT {
            self.telemetry_ring.pop_front();
        }

        self.telemetry_ring.push_back(entry);
        self.snapshot.telemetry = self.telemetry_ring.iter().cloned().collect();
        self.snapshot.overview.updated_at = timestamp_now();
    }

    pub fn transition(&mut self, next: HarnessState) {
        self.snapshot.overview.harness_state = next;
        self.snapshot.overview.robot_mode = match next {
            HarnessState::Boot => RobotMode::Booting,
            HarnessState::Idle => RobotMode::Idle,
            HarnessState::Fault => RobotMode::Faulted,
            HarnessState::TestTouch
            | HarnessState::TestMotor
            | HarnessState::TestDisplay
            | HarnessState::TestAudioInPlaceholder
            | HarnessState::TestAudioOutPlaceholder => RobotMode::Diagnostics,
        };
        self.snapshot.overview.emotion = match next {
            HarnessState::Boot => RobotEmotion::Focused,
            HarnessState::Idle => RobotEmotion::Neutral,
            HarnessState::TestTouch => RobotEmotion::Curious,
            HarnessState::Fault => RobotEmotion::Alert,
            HarnessState::TestMotor
            | HarnessState::TestDisplay
            | HarnessState::TestAudioInPlaceholder
            | HarnessState::TestAudioOutPlaceholder => RobotEmotion::Focused,
        };
        self.snapshot.overview.updated_at = timestamp_now();
    }

    pub fn increment_heartbeat(&mut self) {
        self.snapshot.overview.heartbeat_count += 1;
        self.snapshot.overview.updated_at = timestamp_now();
    }

    pub fn set_fault(&mut self, fault: Option<String>) {
        self.snapshot.overview.fault = fault;
        if self.snapshot.overview.fault.is_some() {
            self.transition(HarnessState::Fault);
            self.apply_motion(
                MotionCommand::Stop,
                0.0,
                0.0,
                ControlSource::Safety,
                "fault forced emergency stop",
            );
        }
    }

    pub fn set_display_message(&mut self, message: impl Into<String>) {
        self.snapshot.sensors.display_message = message.into();
        self.snapshot.overview.updated_at = timestamp_now();
    }

    pub fn set_touch_sensors(&mut self, pet_touch: Option<bool>, record_touch: Option<bool>) {
        if let Some(value) = pet_touch {
            self.snapshot.sensors.pet_touch = value;
        }
        if let Some(value) = record_touch {
            self.snapshot.sensors.record_touch = value;
        }
        self.snapshot.overview.updated_at = timestamp_now();
    }

    pub fn update_gamepad(
        &mut self,
        connected: bool,
        driving_enabled: bool,
        id: Option<String>,
        axes: Vec<f32>,
        buttons: Vec<f32>,
    ) {
        self.snapshot.gamepad = GamepadStatus {
            connected,
            driving_enabled,
            id,
            axes,
            buttons,
        };
        self.snapshot.overview.updated_at = timestamp_now();
    }

    pub fn apply_motion(
        &mut self,
        command: MotionCommand,
        throttle: f32,
        turn: f32,
        source: ControlSource,
        detail: &str,
    ) {
        let (left_output, right_output) = match command {
            MotionCommand::Stop => (0.0, 0.0),
            MotionCommand::Forward => (1.0, 1.0),
            MotionCommand::Backward => (-1.0, -1.0),
            MotionCommand::TurnLeft => (-0.7, 0.7),
            MotionCommand::TurnRight => (0.7, -0.7),
            MotionCommand::ArcadeDrive => {
                let left = (throttle + turn).clamp(-1.0, 1.0);
                let right = (throttle - turn).clamp(-1.0, 1.0);
                (left, right)
            }
        };

        self.snapshot.motors = MotorStatus {
            command,
            source,
            throttle,
            turn,
            left_output,
            right_output,
            gpio: gpio_from_outputs(left_output, right_output),
        };
        self.snapshot.overview.updated_at = timestamp_now();
        self.log("info", "motor", "command_applied", detail);
    }
}

fn gpio_from_outputs(left: f32, right: f32) -> MotorGpioState {
    let (in1, in2) = direction_to_gpio(left);
    let (in3, in4) = direction_to_gpio(right);

    MotorGpioState { in1, in2, in3, in4 }
}

fn direction_to_gpio(value: f32) -> (bool, bool) {
    if value > 0.01 {
        (true, false)
    } else if value < -0.01 {
        (false, true)
    } else {
        (false, false)
    }
}

fn timestamp_now() -> String {
    Local::now().format("%H:%M:%S").to_string()
}
