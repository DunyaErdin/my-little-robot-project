use core::convert::TryInto;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};

use anyhow::{anyhow, Context, Result};
use embedded_svc::{
    http::{server::Request, Headers, Method},
    io::{Read, Write},
    wifi::{self, AccessPointConfiguration, AuthMethod},
};
use esp_idf_hal::modem::Modem;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    http::server::{Configuration as HttpServerConfiguration, EspHttpServer},
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, EspWifi},
};
use log::info;
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    app::state::HarnessState as AppHarnessState,
    control_panel::{
        model::{
            ArcadeDriveRequest, ControlAccess, ControlSource, GamepadStatus, GamepadUpdateRequest,
            HarnessState, InspectionStatus, ManualChecks, ManualChecksRequest, MotionCommand,
            MotorActionRequest, MotorGpioState, MotorStatus, OverviewCard, PinConnectionStatus,
            PinStatus, PinVerificationMethod, RobotEmotion, RobotMode, RobotSnapshot,
            RunTestRequest, SensorStatus, SystemAction, SystemActionRequest, TelemetryEntry,
            TestExecutionState, TestKind, TestReport, TestStatus, TransportStatus,
        },
        web::INDEX_HTML,
    },
    domain::fault::FirmwareFault,
    platform::pins::PIN_MAP,
    ports::telemetry::TelemetryLevel,
};

const CONTROL_PANEL_SSID: &str = "robot-panel-s3";
const CONTROL_PANEL_PASSWORD: &str = "robotpanel123";
const CONTROL_PANEL_CHANNEL: u8 = 6;
const HTTP_STACK_SIZE: usize = 10_240;
const MAX_JSON_BODY_LEN: usize = 2_048;
const TELEMETRY_LIMIT: usize = 128;
const ALL_TESTS: [TestKind; 6] = [
    TestKind::FullHarness,
    TestKind::Touch,
    TestKind::Motor,
    TestKind::Display,
    TestKind::AudioIn,
    TestKind::AudioOut,
];

#[derive(Debug, Clone)]
pub enum PendingPanelCommand {
    RunInspection,
    RunTest(TestKind),
    Motion(MotionCommand, ControlSource),
    System(SystemAction),
}

#[derive(Debug, Clone, Copy)]
pub struct PendingDriveCommand {
    pub throttle: f32,
    pub turn: f32,
    pub source: ControlSource,
}

impl PendingDriveCommand {
    const DEADZONE: f32 = 0.18;

    pub fn new(throttle: f32, turn: f32, source: ControlSource) -> Self {
        Self {
            throttle: throttle.clamp(-1.0, 1.0),
            turn: turn.clamp(-1.0, 1.0),
            source,
        }
    }

    pub fn effective_motion_command(self) -> MotionCommand {
        if self.throttle.abs() < Self::DEADZONE && self.turn.abs() < Self::DEADZONE {
            MotionCommand::Stop
        } else if self.turn.abs() > self.throttle.abs() {
            if self.turn.is_sign_positive() {
                MotionCommand::TurnRight
            } else {
                MotionCommand::TurnLeft
            }
        } else if self.throttle.is_sign_positive() {
            MotionCommand::Forward
        } else {
            MotionCommand::Backward
        }
    }
}

struct PendingControlQueue {
    commands: VecDeque<PendingPanelCommand>,
    drive: Option<PendingDriveCommand>,
}

struct PanelStateStore {
    started_at: Instant,
    snapshot: RobotSnapshot,
    telemetry_ring: VecDeque<TelemetryEntry>,
    pending: PendingControlQueue,
    display_probe_present: Option<bool>,
    display_probe_detail: String,
}

pub struct RemoteControlPanel {
    state: Arc<Mutex<PanelStateStore>>,
    _wifi: BlockingWifi<EspWifi<'static>>,
    _server: EspHttpServer<'static>,
}

impl RemoteControlPanel {
    pub fn start(modem: Modem<'static>) -> Result<Self> {
        let state = Arc::new(Mutex::new(PanelStateStore::new()));
        let sys_loop = EspSystemEventLoop::take().context("failed to acquire system event loop")?;
        let nvs = EspDefaultNvsPartition::take().context("failed to acquire default NVS")?;

        let mut wifi = BlockingWifi::wrap(
            EspWifi::new(modem, sys_loop.clone(), Some(nvs))
                .context("failed to create ESP Wi-Fi driver")?,
            sys_loop,
        )
        .context("failed to wrap Wi-Fi in blocking adapter")?;

        configure_softap(&mut wifi)?;

        let ip_info = wifi
            .wifi()
            .ap_netif()
            .get_ip_info()
            .context("failed to query SoftAP IP address")?;
        let target = format!("http://{}/", ip_info.ip);

        {
            let mut store = lock_store(&state);
            store.snapshot.transport = TransportStatus {
                backend: "esp32_softap_http".to_string(),
                target: target.clone(),
                connected: true,
                ssid: CONTROL_PANEL_SSID.to_string(),
                password_hint: CONTROL_PANEL_PASSWORD.to_string(),
            };
            store.snapshot.sensors.display_message =
                "BAŞLATILIYOR: Wi-Fi kontrol paneli hazır".to_string();
            store.log(
                "info",
                "panel",
                "wifi_ready",
                &format!(
                    "SSID={} ağına bağlanıp {} adresine gidin",
                    CONTROL_PANEL_SSID, target
                ),
            );
        }

        let server = create_http_server(state.clone()).context("failed to start HTTP server")?;

        info!(
            "Wi-Fi control panel ready: SSID={} password={} url={}",
            CONTROL_PANEL_SSID, CONTROL_PANEL_PASSWORD, target
        );

        Ok(Self {
            state,
            _wifi: wifi,
            _server: server,
        })
    }

    pub fn push_log(&self, level: TelemetryLevel, component: &str, action: &str, detail: &str) {
        lock_store(&self.state).log(level_label(level), component, action, detail);
    }

    pub fn set_fault(&self, fault: Option<&FirmwareFault>) {
        let mut store = lock_store(&self.state);
        store.snapshot.overview.fault = fault.map(ToString::to_string);
        if fault.is_some() {
            store.snapshot.overview.harness_state = HarnessState::Fault;
            store.snapshot.overview.robot_mode = RobotMode::Faulted;
            store.snapshot.overview.emotion = RobotEmotion::Alert;
        }
        store.recompute_test_readiness();
        store.refresh_updated_at();
    }

    pub fn sync_state(
        &self,
        state: AppHarnessState,
        mode: crate::domain::robot_mode::RobotMode,
        emotion: crate::domain::emotion::RobotEmotion,
    ) {
        let mut store = lock_store(&self.state);
        store.snapshot.overview.harness_state = HarnessState::from(state);
        store.snapshot.overview.robot_mode = RobotMode::from(mode);
        store.snapshot.overview.emotion = RobotEmotion::from(emotion);
        if state != AppHarnessState::Fault {
            store.snapshot.overview.fault = None;
        }
        store.recompute_test_readiness();
        store.refresh_updated_at();
    }

    pub fn mark_heartbeat(&self) {
        let mut store = lock_store(&self.state);
        store.snapshot.overview.heartbeat_count += 1;
        store.refresh_updated_at();
    }

    pub fn update_touch_inputs(&self, pet: bool, record: bool) {
        lock_store(&self.state).touch_updated(pet, record);
    }

    pub fn set_display_message(&self, message: impl Into<String>) {
        let mut store = lock_store(&self.state);
        store.snapshot.sensors.display_message = message.into();
        store.refresh_updated_at();
    }

    pub fn set_audio_placeholder_status(&self, audio_in_ready: bool, audio_out_ready: bool) {
        let mut store = lock_store(&self.state);
        store.snapshot.sensors.audio_in_placeholder_ready = audio_in_ready;
        store.snapshot.sensors.audio_out_placeholder_ready = audio_out_ready;
        store.refresh_updated_at();
    }

    pub fn update_motion(
        &self,
        command: MotionCommand,
        source: ControlSource,
        throttle: f32,
        turn: f32,
        detail: &str,
    ) {
        let mut store = lock_store(&self.state);
        store.apply_motion(command, source, throttle, turn);
        store.log("info", "motor", "panel_state_sync", detail);
    }

    pub fn block_reason_for_test(&self, test: TestKind) -> Option<String> {
        lock_store(&self.state).test_block_reason(test)
    }

    pub fn motion_block_reason(&self) -> Option<String> {
        lock_store(&self.state).drive_block_reason()
    }

    pub fn begin_inspection_scan(&self) {
        let mut store = lock_store(&self.state);
        let now = store.now_string();
        store.snapshot.inspection.scan_in_progress = true;
        store.snapshot.inspection.scan_completed = false;
        store.snapshot.inspection.post_passed = false;
        store.snapshot.inspection.last_scan_at = now;
        store.snapshot.inspection.last_scan_summary = "Ön kontrol çalışıyor".to_string();
        store.recompute_test_readiness();
        store.refresh_updated_at();
    }

    pub fn complete_inspection_scan(
        &self,
        display_present: bool,
        display_detail: &str,
        summary: &str,
    ) {
        let mut store = lock_store(&self.state);
        let now = store.now_string();
        store.display_probe_present = Some(display_present);
        store.display_probe_detail = display_detail.to_string();
        store.snapshot.sensors.display_ready = display_present;
        store.snapshot.inspection.scan_in_progress = false;
        store.snapshot.inspection.scan_completed = true;
        store.snapshot.inspection.last_scan_at = now;
        store.snapshot.inspection.last_scan_summary = summary.to_string();
        store.rebuild_inspection();
    }

    pub fn fail_inspection_scan(&self, detail: &str) {
        let mut store = lock_store(&self.state);
        let now = store.now_string();
        store.display_probe_present = None;
        store.display_probe_detail = detail.to_string();
        store.snapshot.sensors.display_ready = false;
        store.snapshot.inspection.scan_in_progress = false;
        store.snapshot.inspection.scan_completed = true;
        store.snapshot.inspection.last_scan_at = now;
        store.snapshot.inspection.last_scan_summary = format!("Ön kontrol hatası: {detail}");
        store.rebuild_inspection();
    }

    pub fn mark_test_running(&self, test: TestKind, detail: &str) {
        lock_store(&self.state).set_test_running(test, detail);
    }

    pub fn mark_test_passed(&self, test: TestKind, detail: &str) {
        lock_store(&self.state).set_test_finished(test, TestExecutionState::Passed, detail);
    }

    pub fn mark_test_failed(&self, test: TestKind, detail: &str) {
        lock_store(&self.state).set_test_finished(test, TestExecutionState::Failed, detail);
    }

    pub fn mark_test_blocked(&self, test: TestKind, detail: &str) {
        lock_store(&self.state).set_test_blocked(test, detail);
    }

    pub fn take_pending_command(&self) -> Option<PendingPanelCommand> {
        lock_store(&self.state).pending.commands.pop_front()
    }

    pub fn take_pending_drive(&self) -> Option<PendingDriveCommand> {
        lock_store(&self.state).pending.drive.take()
    }
}

impl PanelStateStore {
    fn new() -> Self {
        let started_at = Instant::now();
        let initial_timestamp = "T+0.000s".to_string();
        let mut store = Self {
            started_at,
            snapshot: RobotSnapshot {
                overview: OverviewCard {
                    harness_state: HarnessState::Boot,
                    robot_mode: RobotMode::Booting,
                    emotion: RobotEmotion::Focused,
                    heartbeat_count: 0,
                    fault: None,
                    updated_at: initial_timestamp.clone(),
                },
                controls: ControlAccess {
                    motion_run_allowed: false,
                    motion_blocked_reason: Some(
                        "Motor sürücüsü kablolaması henüz doğrulanmadı.".to_string(),
                    ),
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
                    pet_touch_seen: false,
                    record_touch_seen: false,
                    display_ready: false,
                    display_message: "BAŞLATILIYOR".to_string(),
                    audio_in_placeholder_ready: false,
                    audio_out_placeholder_ready: false,
                },
                inspection: InspectionStatus {
                    scan_completed: false,
                    scan_in_progress: false,
                    post_passed: false,
                    last_scan_at: initial_timestamp.clone(),
                    last_scan_summary: "Henüz ön kontrol yapılmadı".to_string(),
                    manual_checks: ManualChecks {
                        motor_driver_wired: false,
                        microphone_wired: false,
                        speaker_wired: false,
                    },
                    limitations: default_limitations(),
                    pins: Vec::new(),
                },
                tests: TestStatus {
                    running: None,
                    last_completed: None,
                    reports: default_test_reports(&initial_timestamp),
                },
                gamepad: GamepadStatus {
                    connected: false,
                    driving_enabled: false,
                    id: None,
                    axes: Vec::new(),
                    buttons: Vec::new(),
                },
                transport: TransportStatus {
                    backend: "esp32_softap_http".to_string(),
                    target: "starting".to_string(),
                    connected: false,
                    ssid: CONTROL_PANEL_SSID.to_string(),
                    password_hint: CONTROL_PANEL_PASSWORD.to_string(),
                },
                telemetry: Vec::new(),
            },
            telemetry_ring: VecDeque::with_capacity(TELEMETRY_LIMIT),
            pending: PendingControlQueue {
                commands: VecDeque::new(),
                drive: None,
            },
            display_probe_present: None,
            display_probe_detail: "OLED I2C doğrulaması henüz yapılmadı".to_string(),
        };

        store.rebuild_inspection();
        store
    }

    fn now_string(&self) -> String {
        let elapsed = self.started_at.elapsed();
        format!("T+{}.{:03}s", elapsed.as_secs(), elapsed.subsec_millis())
    }

    fn refresh_updated_at(&mut self) {
        self.snapshot.overview.updated_at = self.now_string();
    }

    fn log(&mut self, level: &str, component: &str, action: &str, detail: &str) {
        let entry = TelemetryEntry {
            timestamp: self.now_string(),
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
        self.refresh_updated_at();
    }

    fn touch_updated(&mut self, pet: bool, record: bool) {
        self.snapshot.sensors.pet_touch = pet;
        self.snapshot.sensors.record_touch = record;
        self.snapshot.sensors.pet_touch_seen |= pet;
        self.snapshot.sensors.record_touch_seen |= record;
        self.rebuild_inspection();
    }

    fn apply_motion(
        &mut self,
        command: MotionCommand,
        source: ControlSource,
        throttle: f32,
        turn: f32,
    ) {
        let (left_output, right_output, gpio) = match command {
            MotionCommand::Stop => (
                0.0,
                0.0,
                MotorGpioState {
                    in1: false,
                    in2: false,
                    in3: false,
                    in4: false,
                },
            ),
            MotionCommand::Forward => (
                1.0,
                1.0,
                MotorGpioState {
                    in1: true,
                    in2: false,
                    in3: true,
                    in4: false,
                },
            ),
            MotionCommand::Backward => (
                -1.0,
                -1.0,
                MotorGpioState {
                    in1: false,
                    in2: true,
                    in3: false,
                    in4: true,
                },
            ),
            MotionCommand::TurnLeft => (
                -1.0,
                1.0,
                MotorGpioState {
                    in1: false,
                    in2: true,
                    in3: true,
                    in4: false,
                },
            ),
            MotionCommand::TurnRight => (
                1.0,
                -1.0,
                MotorGpioState {
                    in1: true,
                    in2: false,
                    in3: false,
                    in4: true,
                },
            ),
            MotionCommand::ArcadeDrive => {
                let left_output = (throttle + turn).clamp(-1.0, 1.0);
                let right_output = (throttle - turn).clamp(-1.0, 1.0);
                (
                    left_output,
                    right_output,
                    gpio_from_outputs(left_output, right_output),
                )
            }
        };

        self.snapshot.motors = MotorStatus {
            command,
            source,
            throttle,
            turn,
            left_output,
            right_output,
            gpio,
        };
        self.rebuild_inspection();
    }

    fn set_test_running(&mut self, test: TestKind, detail: &str) {
        let now = self.now_string();
        self.snapshot.tests.running = Some(test);
        let report = self.report_mut(test);
        report.state = TestExecutionState::Running;
        report.run_allowed = false;
        report.blocked_reason = None;
        report.last_detail = detail.to_string();
        report.last_updated = now;
        self.recompute_test_readiness();
        self.refresh_updated_at();
    }

    fn set_test_finished(&mut self, test: TestKind, state: TestExecutionState, detail: &str) {
        let now = self.now_string();
        self.snapshot.tests.running = None;
        self.snapshot.tests.last_completed = Some(test);
        let report = self.report_mut(test);
        report.state = state;
        report.blocked_reason = None;
        report.last_detail = detail.to_string();
        report.last_updated = now;
        self.recompute_test_readiness();
        self.refresh_updated_at();
    }

    fn set_test_blocked(&mut self, test: TestKind, detail: &str) {
        let now = self.now_string();
        let report = self.report_mut(test);
        report.state = TestExecutionState::Blocked;
        report.last_detail = detail.to_string();
        report.last_updated = now;
        report.blocked_reason = Some(detail.to_string());
        self.recompute_test_readiness();
        self.refresh_updated_at();
    }

    fn report_mut(&mut self, test: TestKind) -> &mut TestReport {
        self.snapshot
            .tests
            .reports
            .iter_mut()
            .find(|report| report.test == test)
            .expect("all tests must have a report entry")
    }

    fn test_block_reason(&self, test: TestKind) -> Option<String> {
        if self.snapshot.overview.harness_state == HarnessState::Fault {
            return Some("Sistem fault durumunda; önce güvenli şekilde sıfırlayın.".to_string());
        }

        if self.snapshot.inspection.scan_in_progress {
            return Some("Ön kontrol taraması sürüyor; tamamlanmasını bekleyin.".to_string());
        }

        if let Some(running) = self.snapshot.tests.running {
            return Some(format!(
                "Şu anda {} çalışıyor; yeni test başlatılamaz.",
                running.label_tr()
            ));
        }

        match test {
            TestKind::Touch => None,
            TestKind::Display => {
                if !self.snapshot.inspection.scan_completed {
                    Some("Önce ön kontrol taraması çalıştırılmalı.".to_string())
                } else if self.display_probe_present != Some(true) {
                    Some("OLED ekran otomatik I2C probunda bulunamadı.".to_string())
                } else {
                    None
                }
            }
            TestKind::Motor => {
                if !self.snapshot.inspection.manual_checks.motor_driver_wired {
                    Some("Motor sürücüsü kablolaması operatör tarafından onaylanmalı.".to_string())
                } else {
                    None
                }
            }
            TestKind::AudioIn => {
                if !self.snapshot.inspection.manual_checks.microphone_wired {
                    Some("Mikrofon I2S kablolaması operatör tarafından onaylanmalı.".to_string())
                } else {
                    None
                }
            }
            TestKind::AudioOut => {
                if !self.snapshot.inspection.manual_checks.speaker_wired {
                    Some(
                        "Hoparlör/amfi I2S kablolaması operatör tarafından onaylanmalı."
                            .to_string(),
                    )
                } else {
                    None
                }
            }
            TestKind::FullHarness => {
                if !self.snapshot.inspection.scan_completed {
                    Some("Tam test için önce ön kontrol taraması çalıştırılmalı.".to_string())
                } else if self.display_probe_present != Some(true) {
                    Some("Tam test için OLED otomatik probu başarılı olmalı.".to_string())
                } else if !self.snapshot.inspection.manual_checks.motor_driver_wired {
                    Some("Tam test için motor sürücüsü kablolaması onaylanmalı.".to_string())
                } else if !self.snapshot.inspection.manual_checks.microphone_wired {
                    Some("Tam test için mikrofon kablolaması onaylanmalı.".to_string())
                } else if !self.snapshot.inspection.manual_checks.speaker_wired {
                    Some("Tam test için hoparlör/amfi kablolaması onaylanmalı.".to_string())
                } else {
                    None
                }
            }
        }
    }

    fn drive_block_reason(&self) -> Option<String> {
        if self.snapshot.overview.harness_state == HarnessState::Fault {
            return Some("Fault durumunda motor komutu gönderilemez.".to_string());
        }

        if self.snapshot.inspection.scan_in_progress {
            return Some("Ön kontrol sürerken manuel hareket komutu gönderilemez.".to_string());
        }

        if self.snapshot.tests.running.is_some() {
            return Some("Test çalışırken manuel motor komutu gönderilemez.".to_string());
        }

        if !self.snapshot.inspection.manual_checks.motor_driver_wired {
            return Some(
                "Motor sürücüsü kablolaması onaylanmadan hareket komutu kilitlidir.".to_string(),
            );
        }

        None
    }

    fn recompute_test_readiness(&mut self) {
        let now = self.now_string();

        for test in ALL_TESTS {
            let blocked_reason = self.test_block_reason(test);
            let report = self.report_mut(test);
            report.run_allowed = blocked_reason.is_none();

            if report.state != TestExecutionState::Running {
                report.blocked_reason = blocked_reason;
            }

            if report.last_updated.is_empty() {
                report.last_updated = now.clone();
            }
        }

        let motion_blocked_reason = self.drive_block_reason();
        self.snapshot.controls.motion_run_allowed = motion_blocked_reason.is_none();
        self.snapshot.controls.motion_blocked_reason = motion_blocked_reason;
    }

    fn rebuild_inspection(&mut self) {
        let display_status = match self.display_probe_present {
            Some(true) => PinConnectionStatus::Present,
            Some(false) => PinConnectionStatus::Absent,
            None => PinConnectionStatus::Unknown,
        };
        let motor_status = if self.snapshot.inspection.manual_checks.motor_driver_wired {
            PinConnectionStatus::ManualConfirmed
        } else {
            PinConnectionStatus::ManualRequired
        };
        let mic_status = if self.snapshot.inspection.manual_checks.microphone_wired {
            PinConnectionStatus::ManualConfirmed
        } else {
            PinConnectionStatus::ManualRequired
        };
        let speaker_status = if self.snapshot.inspection.manual_checks.speaker_wired {
            PinConnectionStatus::ManualConfirmed
        } else {
            PinConnectionStatus::ManualRequired
        };

        self.snapshot.sensors.display_ready = self.display_probe_present == Some(true);
        self.snapshot.inspection.post_passed = self.snapshot.inspection.scan_completed
            && self.display_probe_present == Some(true)
            && self.snapshot.inspection.manual_checks.motor_driver_wired
            && self.snapshot.inspection.manual_checks.microphone_wired
            && self.snapshot.inspection.manual_checks.speaker_wired;
        self.snapshot.inspection.limitations = default_limitations();
        self.snapshot.inspection.pins = vec![
            PinStatus {
                key: "oled_scl".to_string(),
                label: "OLED SCL".to_string(),
                gpio: PIN_MAP.oled_scl,
                role: "I2C saat hattı".to_string(),
                direction: "bus".to_string(),
                signal: "I2C".to_string(),
                connection_status: display_status,
                verification_method: PinVerificationMethod::AutomaticProbe,
                detail: self.display_probe_detail.clone(),
                required_by: vec![
                    TestKind::Display.label_tr().to_string(),
                    TestKind::FullHarness.label_tr().to_string(),
                ],
            },
            PinStatus {
                key: "oled_sda".to_string(),
                label: "OLED SDA".to_string(),
                gpio: PIN_MAP.oled_sda,
                role: "I2C veri hattı".to_string(),
                direction: "bus".to_string(),
                signal: "I2C".to_string(),
                connection_status: display_status,
                verification_method: PinVerificationMethod::AutomaticProbe,
                detail: self.display_probe_detail.clone(),
                required_by: vec![
                    TestKind::Display.label_tr().to_string(),
                    TestKind::FullHarness.label_tr().to_string(),
                ],
            },
            pin_from_touch(
                "pet_touch",
                "Pet Dokunma Sensörü",
                PIN_MAP.pet_touch,
                self.snapshot.sensors.pet_touch,
                self.snapshot.sensors.pet_touch_seen,
            ),
            pin_from_touch(
                "record_touch",
                "Kayıt Dokunma Sensörü",
                PIN_MAP.record_touch,
                self.snapshot.sensors.record_touch,
                self.snapshot.sensors.record_touch_seen,
            ),
            pin_from_motor(
                "motor_in1",
                "Motor IN1",
                PIN_MAP.motor_in1,
                self.snapshot.motors.gpio.in1,
                motor_status,
            ),
            pin_from_motor(
                "motor_in2",
                "Motor IN2",
                PIN_MAP.motor_in2,
                self.snapshot.motors.gpio.in2,
                motor_status,
            ),
            pin_from_motor(
                "motor_in3",
                "Motor IN3",
                PIN_MAP.motor_in3,
                self.snapshot.motors.gpio.in3,
                motor_status,
            ),
            pin_from_motor(
                "motor_in4",
                "Motor IN4",
                PIN_MAP.motor_in4,
                self.snapshot.motors.gpio.in4,
                motor_status,
            ),
            pin_from_manual(
                "mic_sck",
                "Mikrofon SCK",
                PIN_MAP.mic_sck,
                "I2S bit clock",
                mic_status,
                TestKind::AudioIn,
            ),
            pin_from_manual(
                "mic_ws",
                "Mikrofon WS",
                PIN_MAP.mic_ws,
                "I2S word select",
                mic_status,
                TestKind::AudioIn,
            ),
            pin_from_manual(
                "mic_sd",
                "Mikrofon SD",
                PIN_MAP.mic_sd,
                "I2S veri girişi",
                mic_status,
                TestKind::AudioIn,
            ),
            pin_from_manual(
                "speaker_lrc",
                "Hoparlör LRC",
                PIN_MAP.speaker_lrc,
                "I2S left/right clock",
                speaker_status,
                TestKind::AudioOut,
            ),
            pin_from_manual(
                "speaker_bclk",
                "Hoparlör BCLK",
                PIN_MAP.speaker_bclk,
                "I2S bit clock",
                speaker_status,
                TestKind::AudioOut,
            ),
            pin_from_manual(
                "speaker_din",
                "Hoparlör DIN",
                PIN_MAP.speaker_din,
                "I2S veri çıkışı",
                speaker_status,
                TestKind::AudioOut,
            ),
        ];

        self.recompute_test_readiness();
        self.refresh_updated_at();
    }
}

fn configure_softap(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<()> {
    let wifi_configuration = wifi::Configuration::AccessPoint(AccessPointConfiguration {
        ssid: CONTROL_PANEL_SSID.try_into().unwrap(),
        ssid_hidden: false,
        auth_method: AuthMethod::WPA2Personal,
        password: CONTROL_PANEL_PASSWORD.try_into().unwrap(),
        channel: CONTROL_PANEL_CHANNEL,
        ..Default::default()
    });

    wifi.set_configuration(&wifi_configuration)
        .context("failed to apply SoftAP configuration")?;
    wifi.start().context("failed to start SoftAP")?;
    wifi.wait_netif_up()
        .context("SoftAP network interface did not come up")?;

    Ok(())
}

fn create_http_server(state: Arc<Mutex<PanelStateStore>>) -> Result<EspHttpServer<'static>> {
    let mut server = EspHttpServer::new(&HttpServerConfiguration {
        stack_size: HTTP_STACK_SIZE,
        ..Default::default()
    })
    .context("failed to allocate ESP HTTP server")?;

    server.fn_handler::<anyhow::Error, _>("/", Method::Get, move |request| {
        write_html_response(request, INDEX_HTML)
    })?;

    server.fn_handler::<anyhow::Error, _>("/index.html", Method::Get, move |request| {
        write_html_response(request, INDEX_HTML)
    })?;

    server.fn_handler::<anyhow::Error, _>("/favicon.ico", Method::Get, move |request| {
        write_empty_response(request, 204, "No Content")
    })?;

    {
        let state = state.clone();
        server.fn_handler::<anyhow::Error, _>("/api/state", Method::Get, move |request| {
            let snapshot = lock_store(&state).snapshot.clone();
            write_json_response(request, &snapshot)
        })?;
    }

    {
        let state = state.clone();
        server.fn_handler::<anyhow::Error, _>("/api/heartbeat", Method::Post, move |request| {
            let snapshot = lock_store(&state).snapshot.clone();
            write_json_response(request, &snapshot)
        })?;
    }

    {
        let state = state.clone();
        server.fn_handler::<anyhow::Error, _>(
            "/api/inspection/scan",
            Method::Post,
            move |request| {
                let snapshot = {
                    let mut store = lock_store(&state);
                    if store.snapshot.tests.running.is_some()
                        || store.snapshot.inspection.scan_in_progress
                    {
                        store.log(
                            "warn",
                            "inspection",
                            "scan_rejected",
                            "ön kontrol başlatılamadı; sistem meşgul",
                        );
                    } else {
                        let now = store.now_string();
                        store.snapshot.inspection.scan_in_progress = true;
                        store.snapshot.inspection.scan_completed = false;
                        store.snapshot.inspection.post_passed = false;
                        store.snapshot.inspection.last_scan_at = now;
                        store.snapshot.inspection.last_scan_summary =
                            "Ön kontrol kuyruğa alındı".to_string();
                        store
                            .pending
                            .commands
                            .push_back(PendingPanelCommand::RunInspection);
                        store.log(
                            "info",
                            "inspection",
                            "scan_queued",
                            "otomatik ön kontrol kuyruğa alındı",
                        );
                        store.recompute_test_readiness();
                    }

                    store.snapshot.clone()
                };

                write_json_response(request, &snapshot)
            },
        )?;
    }

    {
        let state = state.clone();
        server.fn_handler::<anyhow::Error, _>(
            "/api/inspection/manual",
            Method::Post,
            move |mut request| {
                let request_body: ManualChecksRequest = read_json_body(&mut request)?;
                let snapshot = {
                    let mut store = lock_store(&state);
                    store.snapshot.inspection.manual_checks = ManualChecks {
                        motor_driver_wired: request_body.motor_driver_wired,
                        microphone_wired: request_body.microphone_wired,
                        speaker_wired: request_body.speaker_wired,
                    };
                    store.log(
                        "info",
                        "inspection",
                        "manual_checks_updated",
                        &format!(
                            "motor={} mic={} speaker={}",
                            request_body.motor_driver_wired,
                            request_body.microphone_wired,
                            request_body.speaker_wired
                        ),
                    );
                    store.rebuild_inspection();
                    store.snapshot.clone()
                };

                write_json_response(request, &snapshot)
            },
        )?;
    }

    {
        let state = state.clone();
        server.fn_handler::<anyhow::Error, _>(
            "/api/tests/run",
            Method::Post,
            move |mut request| {
                let request_body: RunTestRequest = read_json_body(&mut request)?;

                let snapshot = {
                    let mut store = lock_store(&state);
                    if let Some(reason) = store.test_block_reason(request_body.test) {
                        store.set_test_blocked(request_body.test, &reason);
                        store.log(
                            "warn",
                            "tests",
                            "blocked",
                            &format!("{} başlatılamadı: {}", request_body.test.label_tr(), reason),
                        );
                    } else {
                        store
                            .pending
                            .commands
                            .push_back(PendingPanelCommand::RunTest(request_body.test));
                        store.log(
                            "info",
                            "tests",
                            "queued",
                            &format!("{} kuyruğa alındı", request_body.test.label_tr()),
                        );
                    }

                    store.snapshot.clone()
                };

                write_json_response(request, &snapshot)
            },
        )?;
    }

    {
        let state = state.clone();
        server.fn_handler::<anyhow::Error, _>(
            "/api/motors/action",
            Method::Post,
            move |mut request| {
                let request_body: MotorActionRequest = read_json_body(&mut request)?;

                let snapshot = {
                    let mut store = lock_store(&state);
                    let can_move = request_body.action == MotionCommand::Stop
                        || store.drive_block_reason().is_none();

                    if !can_move {
                        let reason = store
                            .drive_block_reason()
                            .unwrap_or_else(|| "hareket komutu kilitli".to_string());
                        store.log(
                            "warn",
                            "motor",
                            "command_blocked",
                            &format!(
                                "{} komutu reddedildi: {}",
                                request_body.action.label_tr(),
                                reason
                            ),
                        );
                    } else {
                        store
                            .pending
                            .commands
                            .push_back(PendingPanelCommand::Motion(
                                request_body.action,
                                ControlSource::PanelButton,
                            ));
                        store.log(
                            "info",
                            "motor",
                            "command_queued",
                            &format!("{} komutu kuyruğa alındı", request_body.action.label_tr()),
                        );
                    }

                    store.snapshot.clone()
                };

                write_json_response(request, &snapshot)
            },
        )?;
    }

    {
        let state = state.clone();
        server.fn_handler::<anyhow::Error, _>(
            "/api/motors/arcade",
            Method::Post,
            move |mut request| {
                let request_body: ArcadeDriveRequest = read_json_body(&mut request)?;
                let drive = PendingDriveCommand::new(
                    request_body.throttle,
                    request_body.turn,
                    ControlSource::Slider,
                );
                let effective = drive.effective_motion_command();

                let snapshot = {
                    let mut store = lock_store(&state);
                    let can_move =
                        effective == MotionCommand::Stop || store.drive_block_reason().is_none();

                    if !can_move {
                        let reason = store
                            .drive_block_reason()
                            .unwrap_or_else(|| "arcade sürüş kilitli".to_string());
                        store.log(
                            "warn",
                            "motor",
                            "arcade_blocked",
                            &format!("Arcade sürüş reddedildi: {}", reason),
                        );
                    } else {
                        store.pending.drive = Some(drive);
                        store.log(
                            "info",
                            "motor",
                            "arcade_queued",
                            &format!(
                                "Arcade sürüş kuyruğa alındı throttle={:.2} turn={:.2}",
                                drive.throttle, drive.turn
                            ),
                        );
                    }

                    store.snapshot.clone()
                };

                write_json_response(request, &snapshot)
            },
        )?;
    }

    {
        let state = state.clone();
        server.fn_handler::<anyhow::Error, _>(
            "/api/gamepad",
            Method::Post,
            move |mut request| {
                let request_body: GamepadUpdateRequest = read_json_body(&mut request)?;

                let snapshot = {
                    let mut store = lock_store(&state);
                    store.snapshot.gamepad = GamepadStatus {
                        connected: request_body.connected,
                        driving_enabled: request_body.driving_enabled,
                        id: request_body.id.clone(),
                        axes: request_body.axes.clone(),
                        buttons: request_body.buttons.clone(),
                    };

                    if request_body.connected && request_body.driving_enabled {
                        let throttle = request_body
                            .axes
                            .get(1)
                            .copied()
                            .map(|value| -value)
                            .unwrap_or(0.0);
                        let turn = request_body.axes.first().copied().unwrap_or(0.0);
                        let drive =
                            PendingDriveCommand::new(throttle, turn, ControlSource::Gamepad);
                        let effective = drive.effective_motion_command();
                        let can_move = effective == MotionCommand::Stop
                            || store.drive_block_reason().is_none();

                        if can_move {
                            store.pending.drive = Some(drive);
                        } else {
                            let reason = store
                                .drive_block_reason()
                                .unwrap_or_else(|| "gamepad sürüşü kilitli".to_string());
                            store.log(
                                "warn",
                                "gamepad",
                                "drive_blocked",
                                &format!("Gamepad sürüş isteği reddedildi: {}", reason),
                            );
                        }
                    }

                    store.refresh_updated_at();
                    store.snapshot.clone()
                };

                write_json_response(request, &snapshot)
            },
        )?;
    }

    {
        let state = state.clone();
        server.fn_handler::<anyhow::Error, _>(
            "/api/system",
            Method::Post,
            move |mut request| {
                let request_body: SystemActionRequest = read_json_body(&mut request)?;

                let snapshot = {
                    let mut store = lock_store(&state);
                    store
                        .pending
                        .commands
                        .push_back(PendingPanelCommand::System(request_body.action));
                    store.log(
                        "warn",
                        "system",
                        "command_queued",
                        &format!(
                            "sistem komutu kuyruğa alındı: {}",
                            request_body.action.label_tr()
                        ),
                    );
                    store.snapshot.clone()
                };

                write_json_response(request, &snapshot)
            },
        )?;
    }

    Ok(server)
}

fn read_json_body<C, T>(request: &mut Request<C>) -> Result<T>
where
    C: embedded_svc::http::server::Connection,
    T: DeserializeOwned,
{
    let len = request.content_len().unwrap_or(0) as usize;

    if len > MAX_JSON_BODY_LEN {
        return Err(anyhow!(
            "request body too large: received {len} bytes, limit is {MAX_JSON_BODY_LEN}"
        ));
    }

    let mut body = vec![0_u8; len];
    request
        .read_exact(&mut body)
        .map_err(|error| anyhow!("failed to read HTTP request body: {error:?}"))?;

    serde_json::from_slice(&body).context("failed to parse HTTP JSON body")
}

fn write_empty_response<C>(request: Request<C>, status: u16, message: &'static str) -> Result<()>
where
    C: embedded_svc::http::server::Connection,
{
    let _response = request
        .into_response(status, Some(message), &[("Cache-Control", "no-store")])
        .map_err(|error| anyhow!("failed to open HTTP empty response: {error:?}"))?;
    Ok(())
}

fn write_html_response<C>(request: Request<C>, body: &str) -> Result<()>
where
    C: embedded_svc::http::server::Connection,
{
    let mut response = request
        .into_response(
            200,
            Some("OK"),
            &[
                ("Content-Type", "text/html; charset=utf-8"),
                ("Cache-Control", "no-store"),
            ],
        )
        .map_err(|error| anyhow!("failed to open HTTP HTML response: {error:?}"))?;
    response
        .write_all(body.as_bytes())
        .map_err(|error| anyhow!("failed to write HTTP HTML response body: {error:?}"))?;
    Ok(())
}

fn write_json_response<C, T>(request: Request<C>, value: &T) -> Result<()>
where
    C: embedded_svc::http::server::Connection,
    T: Serialize,
{
    let body = serde_json::to_vec(value).context("failed to serialize HTTP JSON response")?;
    let mut response = request
        .into_response(
            200,
            Some("OK"),
            &[
                ("Content-Type", "application/json; charset=utf-8"),
                ("Cache-Control", "no-store"),
            ],
        )
        .map_err(|error| anyhow!("failed to open HTTP JSON response: {error:?}"))?;
    response
        .write_all(&body)
        .map_err(|error| anyhow!("failed to write HTTP JSON response body: {error:?}"))?;
    Ok(())
}

fn pin_from_touch(
    key: &str,
    label: &str,
    gpio: u8,
    current_high: bool,
    seen_high: bool,
) -> PinStatus {
    let connection_status = if seen_high {
        PinConnectionStatus::Present
    } else {
        PinConnectionStatus::Unknown
    };
    let detail = if seen_high {
        "Bu hat mevcut oturumda en az bir kez HIGH olarak gözlendi; bağlantı yolu doğrulanmış kabul edilir."
    } else {
        "Canlı lojik seviye okunur; sensör tetiklenmedikçe kablo kopukluğu otomatik ayırt edilemez."
    };

    PinStatus {
        key: key.to_string(),
        label: label.to_string(),
        gpio,
        role: "Dijital giriş".to_string(),
        direction: "input".to_string(),
        signal: bool_level(current_high).to_string(),
        connection_status,
        verification_method: PinVerificationMethod::LiveSignal,
        detail: detail.to_string(),
        required_by: vec![
            TestKind::Touch.label_tr().to_string(),
            TestKind::FullHarness.label_tr().to_string(),
        ],
    }
}

fn pin_from_motor(
    key: &str,
    label: &str,
    gpio: u8,
    high: bool,
    connection_status: PinConnectionStatus,
) -> PinStatus {
    PinStatus {
        key: key.to_string(),
        label: label.to_string(),
        gpio,
        role: "H-köprüsü dijital çıkışı".to_string(),
        direction: "output".to_string(),
        signal: bool_level(high).to_string(),
        connection_status,
        verification_method: PinVerificationMethod::ManualAck,
        detail: "GPIO seviyesi canlıdır; sürücü giriş kablosu sürekliliği otomatik ölçülemez."
            .to_string(),
        required_by: vec![
            TestKind::Motor.label_tr().to_string(),
            TestKind::FullHarness.label_tr().to_string(),
        ],
    }
}

fn pin_from_manual(
    key: &str,
    label: &str,
    gpio: u8,
    role: &str,
    connection_status: PinConnectionStatus,
    required_test: TestKind,
) -> PinStatus {
    PinStatus {
        key: key.to_string(),
        label: label.to_string(),
        gpio,
        role: role.to_string(),
        direction: "bus".to_string(),
        signal: "REZERVE".to_string(),
        connection_status,
        verification_method: PinVerificationMethod::ManualAck,
        detail: "Bu hat placeholder aşamasında; fiziksel bağlantı otomatik ölçülemez.".to_string(),
        required_by: vec![
            required_test.label_tr().to_string(),
            TestKind::FullHarness.label_tr().to_string(),
        ],
    }
}

fn default_test_reports(now: &str) -> Vec<TestReport> {
    ALL_TESTS
        .iter()
        .copied()
        .map(|test| TestReport {
            test,
            state: TestExecutionState::Idle,
            run_allowed: false,
            blocked_reason: None,
            last_detail: "Henüz çalıştırılmadı".to_string(),
            last_updated: now.to_string(),
        })
        .collect()
}

fn default_limitations() -> Vec<String> {
    vec![
        "OLED için 0x3C I2C probu otomatik doğrulama yapar.".to_string(),
        "Dokunmatik girişlerde anlık seviye ve oturum boyunca tetiklenme bilgisi izlenir."
            .to_string(),
        "Motor ve I2S kablolarında geri besleme hattı olmadığı için fiziksel bağlantı operatör onayı ister."
            .to_string(),
    ]
}

fn bool_level(value: bool) -> &'static str {
    if value {
        "HIGH"
    } else {
        "LOW"
    }
}

fn level_label(level: TelemetryLevel) -> &'static str {
    match level {
        TelemetryLevel::Info => "info",
        TelemetryLevel::Warn => "warn",
        TelemetryLevel::Error => "error",
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

fn lock_store(state: &Arc<Mutex<PanelStateStore>>) -> MutexGuard<'_, PanelStateStore> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
