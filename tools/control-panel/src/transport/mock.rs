use std::time::Duration;

use tokio::time::sleep;

use crate::{
    model::{
        ArcadeDriveRequest, ControlSource, GamepadUpdateRequest, HarnessState, MotionCommand,
        MotorActionRequest, RobotSnapshot, RunTestRequest, SensorOverrideRequest, SystemAction,
        SystemActionRequest, TestKind,
    },
    store::PanelStore,
};

#[derive(Clone)]
pub struct MockRobotTransport {
    store: PanelStore,
}

impl MockRobotTransport {
    const DEADZONE: f32 = 0.18;
    const MOTOR_STEP_MS: u64 = 350;
    const MOTOR_SETTLE_MS: u64 = 150;

    pub fn new() -> Self {
        Self {
            store: PanelStore::new(),
        }
    }

    pub async fn snapshot(&self) -> RobotSnapshot {
        self.store.snapshot().await
    }

    pub async fn heartbeat(&self) -> RobotSnapshot {
        self.store
            .with_mut(|runtime| {
                runtime.increment_heartbeat();
                runtime.log(
                    "info",
                    "panel",
                    "heartbeat",
                    "UI polling heartbeat refreshed",
                );
            })
            .await;

        self.snapshot().await
    }

    pub async fn apply_motion_action(&self, request: MotorActionRequest) -> RobotSnapshot {
        let detail = format!("action={:?} source=panel_button", request.action);
        self.store
            .with_mut(|runtime| {
                runtime.apply_motion(
                    request.action,
                    0.0,
                    0.0,
                    ControlSource::PanelButton,
                    &detail,
                );
            })
            .await;

        self.snapshot().await
    }

    pub async fn apply_arcade_drive(&self, request: ArcadeDriveRequest) -> RobotSnapshot {
        let throttle = request.throttle.clamp(-1.0, 1.0);
        let turn = request.turn.clamp(-1.0, 1.0);
        let detail = format!("throttle={throttle:.2} turn={turn:.2} source=slider");

        self.store
            .with_mut(|runtime| {
                runtime.apply_motion(
                    MotionCommand::ArcadeDrive,
                    throttle,
                    turn,
                    ControlSource::Slider,
                    &detail,
                );
            })
            .await;

        self.snapshot().await
    }

    pub async fn update_sensors(&self, request: SensorOverrideRequest) -> RobotSnapshot {
        let detail = format!(
            "pet_touch={:?} record_touch={:?}",
            request.pet_touch, request.record_touch
        );

        self.store
            .with_mut(|runtime| {
                runtime.set_touch_sensors(request.pet_touch, request.record_touch);
                runtime.log("info", "sensors", "manual_override", &detail);
            })
            .await;

        self.snapshot().await
    }

    pub async fn update_gamepad(&self, request: GamepadUpdateRequest) -> RobotSnapshot {
        let id = request.id.clone();
        let axes = request.axes.clone();
        let buttons = request.buttons.clone();

        self.store
            .with_mut(|runtime| {
                runtime.update_gamepad(
                    request.connected,
                    request.driving_enabled,
                    id,
                    axes,
                    buttons,
                );
            })
            .await;

        if request.connected && request.driving_enabled {
            let throttle = request
                .axes
                .get(1)
                .copied()
                .map(|value| -value)
                .unwrap_or(0.0);
            let turn = request.axes.first().copied().unwrap_or(0.0);

            if throttle.abs() < Self::DEADZONE && turn.abs() < Self::DEADZONE {
                self.store
                    .with_mut(|runtime| {
                        runtime.apply_motion(
                            MotionCommand::Stop,
                            0.0,
                            0.0,
                            ControlSource::Gamepad,
                            "gamepad within deadzone",
                        );
                    })
                    .await;
            } else {
                let detail = format!("throttle={throttle:.2} turn={turn:.2} source=gamepad");
                self.store
                    .with_mut(|runtime| {
                        runtime.apply_motion(
                            MotionCommand::ArcadeDrive,
                            throttle,
                            turn,
                            ControlSource::Gamepad,
                            &detail,
                        );
                    })
                    .await;
            }
        }

        self.snapshot().await
    }

    pub async fn apply_system_action(&self, request: SystemActionRequest) -> RobotSnapshot {
        self.store
            .with_mut(|runtime| match request.action {
                SystemAction::ResetIdle | SystemAction::ClearFault => {
                    runtime.set_fault(None);
                    runtime.transition(HarnessState::Idle);
                    runtime.set_display_message("IDLE: localhost panel armed");
                    runtime.log("info", "system", "clear_fault", "returned robot to idle");
                }
                SystemAction::InjectFault => {
                    runtime.set_fault(Some(
                        "manual fault injected from localhost panel".to_string(),
                    ));
                    runtime.log(
                        "error",
                        "system",
                        "inject_fault",
                        "operator injected a fault",
                    );
                }
                SystemAction::EmergencyStop => {
                    runtime.apply_motion(
                        MotionCommand::Stop,
                        0.0,
                        0.0,
                        ControlSource::Safety,
                        "manual emergency stop from localhost panel",
                    );
                    runtime.log("warn", "system", "emergency_stop", "motors forced to stop");
                }
            })
            .await;

        self.snapshot().await
    }

    pub async fn run_test(&self, request: RunTestRequest) -> RobotSnapshot {
        let this = self.clone();
        tokio::spawn(async move {
            this.execute_test(request.test).await;
        });

        self.snapshot().await
    }

    async fn execute_test(&self, test: TestKind) {
        let already_running = self
            .store
            .with_mut(|runtime| {
                if runtime.snapshot.tests.running.is_some() {
                    runtime.log(
                        "warn",
                        "tests",
                        "busy",
                        "ignored test request while another test was running",
                    );
                    true
                } else {
                    runtime.snapshot.tests.running = Some(test);
                    runtime.log("info", "tests", "start", &format!("test={test:?}"));
                    false
                }
            })
            .await;

        if already_running {
            return;
        }

        match test {
            TestKind::FullHarness => {
                self.execute_touch_test().await;
                self.execute_display_test().await;
                self.execute_motor_test().await;
                self.execute_audio_in_test().await;
                self.execute_audio_out_test().await;
            }
            TestKind::Touch => self.execute_touch_test().await,
            TestKind::Motor => self.execute_motor_test().await,
            TestKind::Display => self.execute_display_test().await,
            TestKind::AudioIn => self.execute_audio_in_test().await,
            TestKind::AudioOut => self.execute_audio_out_test().await,
        }

        self.store
            .with_mut(|runtime| {
                runtime.snapshot.tests.running = None;
                runtime.snapshot.tests.last_completed = Some(test);
                if runtime.snapshot.overview.fault.is_none() {
                    runtime.transition(HarnessState::Idle);
                }
                runtime.log("info", "tests", "complete", &format!("test={test:?}"));
            })
            .await;
    }

    async fn execute_touch_test(&self) {
        self.store
            .with_mut(|runtime| {
                runtime.transition(HarnessState::TestTouch);
                let detail = format!(
                    "pet_touch={} record_touch={}",
                    runtime.snapshot.sensors.pet_touch, runtime.snapshot.sensors.record_touch
                );
                runtime.log("info", "touch", "test_snapshot", &detail);
            })
            .await;

        sleep(Duration::from_millis(120)).await;
    }

    async fn execute_display_test(&self) {
        self.store
            .with_mut(|runtime| {
                runtime.transition(HarnessState::TestDisplay);
                runtime.set_display_message("DISPLAY TEST: placeholder frame rendered");
                runtime.log(
                    "info",
                    "display",
                    "test_frame",
                    "SSD1306 placeholder test frame requested",
                );
            })
            .await;

        sleep(Duration::from_millis(120)).await;
    }

    async fn execute_audio_in_test(&self) {
        self.store
            .with_mut(|runtime| {
                runtime.transition(HarnessState::TestAudioInPlaceholder);
                runtime.log(
                    "warn",
                    "audio_in",
                    "placeholder_ready",
                    "I2S microphone path reserved but no capture stream is connected yet",
                );
            })
            .await;

        sleep(Duration::from_millis(120)).await;
    }

    async fn execute_audio_out_test(&self) {
        self.store
            .with_mut(|runtime| {
                runtime.transition(HarnessState::TestAudioOutPlaceholder);
                runtime.log(
                    "warn",
                    "audio_out",
                    "placeholder_ready",
                    "I2S speaker path reserved but no playback stream is connected yet",
                );
            })
            .await;

        sleep(Duration::from_millis(120)).await;
    }

    async fn execute_motor_test(&self) {
        self.store
            .with_mut(|runtime| {
                runtime.transition(HarnessState::TestMotor);
                runtime.log(
                    "info",
                    "motor",
                    "test_begin",
                    "running deterministic forward/backward/turn sequence",
                );
            })
            .await;

        self.motor_step(MotionCommand::Forward, "forward step")
            .await;
        self.motor_step(MotionCommand::Backward, "backward step")
            .await;
        self.motor_step(MotionCommand::TurnLeft, "turn_left step")
            .await;
        self.motor_step(MotionCommand::TurnRight, "turn_right step")
            .await;
    }

    async fn motor_step(&self, command: MotionCommand, detail: &str) {
        self.store
            .with_mut(|runtime| {
                runtime.apply_motion(
                    MotionCommand::Stop,
                    0.0,
                    0.0,
                    ControlSource::TestHarness,
                    "test harness settle stop",
                );
            })
            .await;
        sleep(Duration::from_millis(Self::MOTOR_SETTLE_MS)).await;

        self.store
            .with_mut(|runtime| {
                runtime.apply_motion(command, 0.0, 0.0, ControlSource::TestHarness, detail);
            })
            .await;
        sleep(Duration::from_millis(Self::MOTOR_STEP_MS)).await;

        self.store
            .with_mut(|runtime| {
                runtime.apply_motion(
                    MotionCommand::Stop,
                    0.0,
                    0.0,
                    ControlSource::TestHarness,
                    "test harness explicit stop",
                );
            })
            .await;
        sleep(Duration::from_millis(Self::MOTOR_SETTLE_MS)).await;
    }
}
