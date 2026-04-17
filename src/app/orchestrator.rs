use std::time::Duration;

use anyhow::{Context, Result};
use esp_idf_hal::delay::FreeRtos;

use crate::{
    adapters::{MotorGpioAdapter, OledDisplayAdapter, SerialTelemetry, TouchGpioAdapter},
    app::{
        events::AppEvent,
        state::{AppState, HarnessState},
    },
    control_panel::{
        model::{ControlSource, MotionCommand, SystemAction, TestKind},
        PendingDriveCommand, PendingPanelCommand, RemoteControlPanel,
    },
    domain::fault::FirmwareFault,
    platform::{
        board::{Board, BoardAudioIn, BoardAudioOut},
        pins::PIN_MAP,
    },
    ports::{
        AudioInPort, AudioOutPort, DisplayPort, MotionPort, TelemetryLevel, TelemetryPort,
        TouchPort, TouchSnapshot,
    },
};

pub struct Orchestrator {
    display: OledDisplayAdapter,
    motion: MotorGpioAdapter,
    touch: TouchGpioAdapter,
    audio_in: BoardAudioIn,
    audio_out: BoardAudioOut,
    telemetry: SerialTelemetry,
    remote_panel: RemoteControlPanel,
    state: AppState,
}

impl Orchestrator {
    const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
    const IDLE_POLL_INTERVAL_MS: u32 = 100;
    const MOTOR_STEP_DELAY_MS: u32 = 350;
    const MOTOR_SETTLE_DELAY_MS: u32 = 150;
    const DISPLAY_TEST_HOLD_MS: u32 = 1_500;
    const FAULT_LOG_INTERVAL_MS: u32 = 1_000;

    pub fn new(board: Board) -> Result<Self> {
        let Board {
            display,
            motion,
            touch,
            audio_in,
            audio_out,
            telemetry,
            modem,
        } = board;

        let remote_panel = RemoteControlPanel::start(modem)
            .context("failed to start ESP32 SoftAP control panel")?;

        Ok(Self {
            display,
            motion,
            touch,
            audio_in,
            audio_out,
            telemetry,
            remote_panel,
            state: AppState::new(),
        })
    }

    pub fn run(mut self) -> ! {
        if let Err(error) = self.boot_sequence() {
            let fault = FirmwareFault::runtime(error.to_string());
            self.emit_event(AppEvent::FaultDetected(fault.clone()));
            self.enter_fault(fault);
        }

        loop {
            if let Err(error) = self.idle_tick() {
                let fault = FirmwareFault::runtime(error.to_string());
                self.emit_event(AppEvent::FaultDetected(fault.clone()));
                self.enter_fault(fault);
            }

            FreeRtos::delay_ms(Self::IDLE_POLL_INTERVAL_MS);
        }
    }

    fn boot_sequence(&mut self) -> Result<()> {
        self.log_event(
            TelemetryLevel::Info,
            "boot",
            "start",
            &format!("pin_map={PIN_MAP}"),
        );
        self.log_event(
            TelemetryLevel::Info,
            "touch",
            "assumption",
            "digital active-high inputs with pulldown bias",
        );

        self.display
            .show_status("BOOT", "robot harness starting")
            .context("failed to publish boot display status")?;
        self.remote_panel
            .set_display_message("BOOT: robot harness starting");
        self.sync_remote_state();

        self.run_audio_in_placeholder_test()?;
        self.run_audio_out_placeholder_test()?;

        self.display
            .show_status("IDLE", "pet=display record=motor")
            .context("failed to publish idle display status")?;
        self.remote_panel
            .set_display_message("IDLE: pet=display record=motor");

        self.transition(HarnessState::Idle);
        self.emit_event(AppEvent::BootCompleted);

        Ok(())
    }

    fn idle_tick(&mut self) -> Result<()> {
        self.log_heartbeat_if_due();

        if self.process_remote_panel_commands()? {
            return Ok(());
        }

        let snapshot = self
            .touch
            .read_snapshot()
            .context("failed to sample touch inputs")?;
        self.remote_panel
            .update_touch_inputs(snapshot.pet, snapshot.record);

        if snapshot.any_triggered() {
            self.run_touch_test(snapshot)?;
        }

        Ok(())
    }

    fn log_heartbeat_if_due(&mut self) {
        if self.state.heartbeat_due(Self::HEARTBEAT_INTERVAL) {
            self.log_heartbeat();
            self.state.mark_heartbeat();
            self.remote_panel.mark_heartbeat();
        }
    }

    fn run_touch_test(&mut self, snapshot: TouchSnapshot) -> Result<()> {
        self.transition(HarnessState::TestTouch);
        self.log_test_started("touch");
        self.log_test_succeeded("touch", &snapshot.describe());

        if snapshot.pet {
            self.emit_event(AppEvent::PetTouchTriggered);
            if self.ensure_test_allowed(TestKind::Display, "display")? {
                self.run_display_test()?;
            }
        }

        if snapshot.record {
            self.emit_event(AppEvent::RecordTouchTriggered);
            if self.ensure_test_allowed(TestKind::Motor, "motor")? {
                self.run_motor_test()?;
            }
        }

        self.transition(HarnessState::Idle);
        Ok(())
    }

    fn run_display_test(&mut self) -> Result<()> {
        self.transition(HarnessState::TestDisplay);
        self.log_test_started("display");
        self.remote_panel
            .set_display_message("DISPLAY TEST: smiling face requested");

        let result = self
            .display
            .run_test_frame()
            .context("display placeholder test API returned an error");

        match result {
            Ok(()) => {
                self.log_test_succeeded(
                    "display",
                    "OLED smiling face rendered via SSD1306 buffered graphics",
                );
                self.emit_event(AppEvent::DisplayTestCompleted);
                self.remote_panel
                    .set_display_message("DISPLAY TEST: smiling face rendered");
                FreeRtos::delay_ms(Self::DISPLAY_TEST_HOLD_MS);
                Ok(())
            }
            Err(error) => {
                let fault = FirmwareFault::runtime(format!("display test failed: {error}"));
                self.log_test_failed("display", &fault);
                Err(error)
            }
        }
    }

    fn run_motor_test(&mut self) -> Result<()> {
        self.transition(HarnessState::TestMotor);
        self.log_test_started("motor");

        let result = (|| -> Result<()> {
            self.execute_motion_step("forward", MotionCommand::Forward, |motion| motion.forward())?;
            self.execute_motion_step("backward", MotionCommand::Backward, |motion| {
                motion.backward()
            })?;
            self.execute_motion_step("turn_left", MotionCommand::TurnLeft, |motion| {
                motion.turn_left()
            })?;
            self.execute_motion_step("turn_right", MotionCommand::TurnRight, |motion| {
                motion.turn_right()
            })?;
            Ok(())
        })();

        match result {
            Ok(()) => {
                self.log_test_succeeded("motor", "full deterministic drive sequence completed");
                self.emit_event(AppEvent::MotorTestCompleted);
                Ok(())
            }
            Err(error) => {
                let fault = FirmwareFault::runtime(format!("motor test failed: {error}"));
                self.log_test_failed("motor", &fault);
                Err(error)
            }
        }
    }

    fn run_audio_in_placeholder_test(&mut self) -> Result<()> {
        self.transition(HarnessState::TestAudioInPlaceholder);
        self.log_test_started("audio_in_placeholder");

        let result = self
            .audio_in
            .announce_placeholder_ready()
            .context("audio input placeholder readiness hook failed");

        match result {
            Ok(()) => {
                self.log_test_succeeded("audio_in_placeholder", self.audio_in.readiness_note());
                self.remote_panel.set_audio_placeholder_status(true, false);
                self.emit_event(AppEvent::AudioInPlaceholderReady);
                Ok(())
            }
            Err(error) => {
                let fault =
                    FirmwareFault::runtime(format!("audio input placeholder failed: {error}"));
                self.log_test_failed("audio_in_placeholder", &fault);
                Err(error)
            }
        }
    }

    fn run_audio_out_placeholder_test(&mut self) -> Result<()> {
        self.transition(HarnessState::TestAudioOutPlaceholder);
        self.log_test_started("audio_out_placeholder");

        let result = self
            .audio_out
            .announce_placeholder_ready()
            .context("audio output placeholder readiness hook failed");

        match result {
            Ok(()) => {
                self.log_test_succeeded("audio_out_placeholder", self.audio_out.readiness_note());
                self.remote_panel.set_audio_placeholder_status(true, true);
                self.emit_event(AppEvent::AudioOutPlaceholderReady);
                Ok(())
            }
            Err(error) => {
                let fault =
                    FirmwareFault::runtime(format!("audio output placeholder failed: {error}"));
                self.log_test_failed("audio_out_placeholder", &fault);
                Err(error)
            }
        }
    }

    fn execute_motion_step<F>(
        &mut self,
        step_name: &str,
        motion_command: MotionCommand,
        command: F,
    ) -> Result<()>
    where
        F: FnOnce(&mut MotorGpioAdapter) -> Result<()>,
    {
        self.log_event(
            TelemetryLevel::Info,
            "motor",
            step_name,
            "starting motion step",
        );

        self.motion
            .stop()
            .with_context(|| format!("failed to stop motor before step `{step_name}`"))?;
        self.remote_panel.update_motion(
            MotionCommand::Stop,
            ControlSource::TestHarness,
            0.0,
            0.0,
            &format!("test step={step_name} pre-stop"),
        );
        FreeRtos::delay_ms(Self::MOTOR_SETTLE_DELAY_MS);

        command(&mut self.motion)
            .with_context(|| format!("failed to execute motor step `{step_name}`"))?;
        self.remote_panel.update_motion(
            motion_command,
            ControlSource::TestHarness,
            0.0,
            0.0,
            &format!("test step={step_name} active"),
        );
        self.log_event(
            TelemetryLevel::Info,
            "motor",
            step_name,
            "motion step active",
        );

        FreeRtos::delay_ms(Self::MOTOR_STEP_DELAY_MS);

        self.motion
            .stop()
            .with_context(|| format!("failed to stop motor after step `{step_name}`"))?;
        self.remote_panel.update_motion(
            MotionCommand::Stop,
            ControlSource::TestHarness,
            0.0,
            0.0,
            &format!("test step={step_name} completed"),
        );
        self.log_event(
            TelemetryLevel::Info,
            "motor",
            "stop",
            &format!("completed step={step_name}"),
        );

        FreeRtos::delay_ms(Self::MOTOR_SETTLE_DELAY_MS);
        Ok(())
    }

    fn transition(&mut self, next: HarnessState) {
        let previous = self.state.harness_state();
        self.state.transition_to(next);
        self.sync_remote_state();

        let detail = format!(
            "from={} to={} mode={} emotion={}",
            previous.as_str(),
            next.as_str(),
            self.state.robot_mode().as_str(),
            self.state.emotion().as_str(),
        );

        self.log_event(TelemetryLevel::Info, "state", "transition", &detail);
    }

    fn emit_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::BootCompleted => self.log_event(
                TelemetryLevel::Info,
                "app",
                "boot_completed",
                "orchestrator entered idle state",
            ),
            AppEvent::PetTouchTriggered => self.log_event(
                TelemetryLevel::Info,
                "touch",
                "pet_triggered",
                "dispatching display test",
            ),
            AppEvent::RecordTouchTriggered => self.log_event(
                TelemetryLevel::Info,
                "touch",
                "record_triggered",
                "dispatching motor test",
            ),
            AppEvent::DisplayTestCompleted => self.log_event(
                TelemetryLevel::Info,
                "display",
                "test_completed",
                "placeholder frame request completed",
            ),
            AppEvent::MotorTestCompleted => self.log_event(
                TelemetryLevel::Info,
                "motor",
                "test_completed",
                "sequence finished with explicit stops between steps",
            ),
            AppEvent::AudioInPlaceholderReady => self.log_event(
                TelemetryLevel::Warn,
                "audio_in",
                "placeholder_ready",
                self.audio_in.readiness_note(),
            ),
            AppEvent::AudioOutPlaceholderReady => self.log_event(
                TelemetryLevel::Warn,
                "audio_out",
                "placeholder_ready",
                self.audio_out.readiness_note(),
            ),
            AppEvent::FaultDetected(fault) => {
                let detail = fault.to_string();
                self.log_event(TelemetryLevel::Error, "app", "fault_detected", &detail);
            }
        }
    }

    fn enter_fault(mut self, fault: FirmwareFault) -> ! {
        self.state.set_fault(fault.clone());
        self.remote_panel.set_fault(Some(&fault));
        self.sync_remote_state();

        if let Err(stop_error) = self.motion.stop() {
            self.log_event(
                TelemetryLevel::Error,
                "motor",
                "emergency_stop_failed",
                &stop_error.to_string(),
            );
        }
        self.remote_panel.update_motion(
            MotionCommand::Stop,
            ControlSource::Safety,
            0.0,
            0.0,
            "fault forced motor stop",
        );

        self.log_fault(&fault);

        loop {
            let active_fault = self
                .state
                .last_fault()
                .cloned()
                .unwrap_or_else(|| fault.clone());
            self.log_fault(&active_fault);
            FreeRtos::delay_ms(Self::FAULT_LOG_INTERVAL_MS);
        }
    }

    fn process_remote_panel_commands(&mut self) -> Result<bool> {
        if let Some(command) = self.remote_panel.take_pending_command() {
            match command {
                PendingPanelCommand::RunInspection => self.run_inspection_scan_from_panel()?,
                PendingPanelCommand::RunTest(TestKind::FullHarness) => {
                    self.run_full_harness_from_panel()?
                }
                PendingPanelCommand::RunTest(TestKind::Touch) => {
                    self.run_touch_snapshot_from_panel()?;
                }
                PendingPanelCommand::RunTest(TestKind::Motor) => {
                    self.run_motor_test_from_panel()?
                }
                PendingPanelCommand::RunTest(TestKind::Display) => {
                    self.run_display_test_from_panel()?
                }
                PendingPanelCommand::RunTest(TestKind::AudioIn) => {
                    self.run_audio_in_test_from_panel()?
                }
                PendingPanelCommand::RunTest(TestKind::AudioOut) => {
                    self.run_audio_out_test_from_panel()?
                }
                PendingPanelCommand::Motion(command, source) => {
                    self.apply_remote_motion_command(command, source)?
                }
                PendingPanelCommand::System(SystemAction::EmergencyStop) => {
                    self.apply_remote_motion_command(MotionCommand::Stop, ControlSource::Safety)?
                }
                PendingPanelCommand::System(SystemAction::ResetIdle) => {
                    self.apply_remote_reset_idle()?
                }
            }

            return Ok(true);
        }

        if let Some(drive) = self.remote_panel.take_pending_drive() {
            self.apply_remote_drive_command(drive)?;
            return Ok(true);
        }

        Ok(false)
    }

    fn run_inspection_scan_from_panel(&mut self) -> Result<()> {
        self.remote_panel.begin_inspection_scan();

        let result = (|| -> Result<()> {
            let touch_snapshot = self
                .touch
                .read_snapshot()
                .context("failed to sample touch inputs during inspection scan")?;
            self.remote_panel
                .update_touch_inputs(touch_snapshot.pet, touch_snapshot.record);

            let display_present = self
                .display
                .probe_presence()
                .context("failed to probe OLED over I2C")?;

            let display_detail = if display_present {
                "OLED 0x3C adresi I2C probuna cevap verdi"
            } else {
                "OLED 0x3C adresi I2C probuna cevap vermedi"
            };
            let summary = format!(
                "Tarama tamamlandı: oled_present={} pet_level={} record_level={}",
                display_present, touch_snapshot.pet, touch_snapshot.record
            );

            self.remote_panel
                .complete_inspection_scan(display_present, display_detail, &summary);
            self.log_event(
                TelemetryLevel::Info,
                "inspection",
                "scan_complete",
                &summary,
            );
            Ok(())
        })();

        if let Err(error) = result {
            let detail = error.to_string();
            self.remote_panel.fail_inspection_scan(&detail);
            self.log_event(TelemetryLevel::Warn, "inspection", "scan_failed", &detail);
        }

        Ok(())
    }

    fn run_full_harness_from_panel(&mut self) -> Result<()> {
        self.remote_panel
            .mark_test_running(TestKind::FullHarness, "Tam test harnesi başlatıldı");

        let result = (|| -> Result<bool> {
            let touch_ok = self.run_touch_snapshot_from_panel()?;
            self.run_display_test_from_panel()?;
            self.run_motor_test_from_panel()?;
            self.run_audio_in_test_from_panel()?;
            self.run_audio_out_test_from_panel()?;
            Ok(touch_ok)
        })();

        match result {
            Ok(true) => {
                self.remote_panel.mark_test_passed(
                    TestKind::FullHarness,
                    "Tüm adımlar planlandığı gibi tamamlandı",
                );
                self.transition(HarnessState::Idle);
                Ok(())
            }
            Ok(false) => {
                self.remote_panel.mark_test_failed(
                    TestKind::FullHarness,
                    "Tam test bitti ancak dokunmatik doğrulama başarısız oldu",
                );
                self.transition(HarnessState::Idle);
                Ok(())
            }
            Err(error) => {
                self.remote_panel.mark_test_failed(
                    TestKind::FullHarness,
                    &format!("Tam test çalışma zamanı hatası: {error}"),
                );
                Err(error)
            }
        }
    }

    fn run_touch_snapshot_from_panel(&mut self) -> Result<bool> {
        const TOUCH_TEST_WINDOW_MS: u32 = 10_000;
        const TOUCH_TEST_POLL_MS: u32 = 100;

        self.remote_panel.mark_test_running(
            TestKind::Touch,
            "10 saniyelik dokunmatik doğrulama penceresi başlatıldı",
        );
        self.transition(HarnessState::TestTouch);

        let mut pet_seen = false;
        let mut record_seen = false;

        for _ in 0..(TOUCH_TEST_WINDOW_MS / TOUCH_TEST_POLL_MS) {
            let snapshot = self
                .touch
                .read_snapshot()
                .context("failed to sample touch inputs during guided touch test")?;
            self.remote_panel
                .update_touch_inputs(snapshot.pet, snapshot.record);

            pet_seen |= snapshot.pet;
            record_seen |= snapshot.record;

            if pet_seen && record_seen {
                break;
            }

            FreeRtos::delay_ms(TOUCH_TEST_POLL_MS);
        }

        let passed = pet_seen && record_seen;
        let detail = format!("pet_seen={} record_seen={}", pet_seen, record_seen);
        self.log_event(TelemetryLevel::Info, "touch", "panel_guided_test", &detail);

        if passed {
            self.remote_panel.mark_test_passed(
                TestKind::Touch,
                "Her iki dokunmatik giriş de test penceresi içinde algılandı",
            );
        } else {
            self.remote_panel.mark_test_failed(
                TestKind::Touch,
                "10 saniye içinde her iki dokunmatik giriş doğrulanamadı",
            );
        }

        self.transition(HarnessState::Idle);
        Ok(passed)
    }

    fn run_display_test_from_panel(&mut self) -> Result<()> {
        self.run_top_level_panel_test(
            TestKind::Display,
            "Panel üzerinden ekran testi başlatıldı",
            "OLED gülen yüz testi başarıyla tamamlandı",
            |this| this.run_display_test(),
        )
    }

    fn run_motor_test_from_panel(&mut self) -> Result<()> {
        self.run_top_level_panel_test(
            TestKind::Motor,
            "Panel üzerinden motor testi başlatıldı",
            "Motor test dizisi başarıyla tamamlandı",
            |this| this.run_motor_test(),
        )
    }

    fn run_audio_in_test_from_panel(&mut self) -> Result<()> {
        self.run_top_level_panel_test(
            TestKind::AudioIn,
            "Panel üzerinden mikrofon hazırlık testi başlatıldı",
            "Mikrofon placeholder hazırlık testi başarılı",
            |this| this.run_audio_in_placeholder_test(),
        )
    }

    fn run_audio_out_test_from_panel(&mut self) -> Result<()> {
        self.run_top_level_panel_test(
            TestKind::AudioOut,
            "Panel üzerinden hoparlör hazırlık testi başlatıldı",
            "Hoparlör placeholder hazırlık testi başarılı",
            |this| this.run_audio_out_placeholder_test(),
        )
    }

    fn run_top_level_panel_test<F>(
        &mut self,
        test: TestKind,
        start_detail: &str,
        success_detail: &str,
        action: F,
    ) -> Result<()>
    where
        F: FnOnce(&mut Self) -> Result<()>,
    {
        if !self.ensure_test_allowed(test, "tests")? {
            return Ok(());
        }

        self.remote_panel.mark_test_running(test, start_detail);
        let result = action(self);

        match result {
            Ok(()) => {
                self.remote_panel.mark_test_passed(test, success_detail);
                self.transition(HarnessState::Idle);
                Ok(())
            }
            Err(error) => {
                self.remote_panel.mark_test_failed(
                    test,
                    &format!("{} çalışma zamanı hatası: {error}", test.label_tr()),
                );
                Err(error)
            }
        }
    }

    fn apply_remote_motion_command(
        &mut self,
        command: MotionCommand,
        source: ControlSource,
    ) -> Result<()> {
        if command != MotionCommand::Stop {
            if let Some(reason) = self.remote_panel.motion_block_reason() {
                self.log_event(
                    TelemetryLevel::Warn,
                    "motor",
                    "panel_command_blocked",
                    &format!("command={} reason={}", command.as_str(), reason),
                );
                return Ok(());
            }
        }

        match command {
            MotionCommand::Stop => self
                .motion
                .stop()
                .context("failed to stop motors from panel")?,
            MotionCommand::Forward => self
                .motion
                .forward()
                .context("failed to drive forward from panel")?,
            MotionCommand::Backward => self
                .motion
                .backward()
                .context("failed to drive backward from panel")?,
            MotionCommand::TurnLeft => self
                .motion
                .turn_left()
                .context("failed to turn left from panel")?,
            MotionCommand::TurnRight => self
                .motion
                .turn_right()
                .context("failed to turn right from panel")?,
            MotionCommand::ArcadeDrive => self
                .motion
                .stop()
                .context("failed to stop motors from arcade request")?,
        }

        self.remote_panel.update_motion(
            command,
            source,
            0.0,
            0.0,
            &format!(
                "panel motion command={} source={}",
                command.as_str(),
                source.as_str()
            ),
        );
        self.log_event(
            TelemetryLevel::Info,
            "motor",
            "panel_command",
            &format!("command={} source={}", command.as_str(), source.as_str()),
        );
        Ok(())
    }

    fn apply_remote_drive_command(&mut self, drive: PendingDriveCommand) -> Result<()> {
        let effective = drive.effective_motion_command();
        if effective != MotionCommand::Stop {
            if let Some(reason) = self.remote_panel.motion_block_reason() {
                self.log_event(
                    TelemetryLevel::Warn,
                    "motor",
                    "arcade_command_blocked",
                    &format!(
                        "effective_command={} throttle={:.2} turn={:.2} reason={}",
                        effective.as_str(),
                        drive.throttle,
                        drive.turn,
                        reason
                    ),
                );
                return Ok(());
            }
        }

        self.apply_remote_motion_command(effective, drive.source)?;
        self.remote_panel.update_motion(
            effective,
            drive.source,
            drive.throttle,
            drive.turn,
            &format!(
                "arcade request mapped to digital command={} throttle={:.2} turn={:.2}",
                effective.as_str(),
                drive.throttle,
                drive.turn,
            ),
        );
        Ok(())
    }

    fn apply_remote_reset_idle(&mut self) -> Result<()> {
        self.motion
            .stop()
            .context("failed to stop motors while resetting idle")?;
        self.display
            .show_status("IDLE", "pet=display record=motor")
            .context("failed to publish idle status after reset")?;
        self.remote_panel
            .set_display_message("IDLE: panel reset requested");
        self.remote_panel.update_motion(
            MotionCommand::Stop,
            ControlSource::Safety,
            0.0,
            0.0,
            "panel reset to idle",
        );
        self.transition(HarnessState::Idle);
        self.log_event(
            TelemetryLevel::Warn,
            "system",
            "reset_idle",
            "panel requested idle reset and motor stop",
        );
        Ok(())
    }

    fn sync_remote_state(&self) {
        self.remote_panel.sync_state(
            self.state.harness_state(),
            self.state.robot_mode(),
            self.state.emotion(),
        );
    }

    fn log_event(&mut self, level: TelemetryLevel, component: &str, action: &str, detail: &str) {
        self.telemetry.log_event(level, component, action, detail);
        self.remote_panel.push_log(level, component, action, detail);
    }

    fn ensure_test_allowed(&mut self, test: TestKind, component: &str) -> Result<bool> {
        if let Some(reason) = self.remote_panel.block_reason_for_test(test) {
            self.remote_panel.mark_test_blocked(test, &reason);
            self.log_event(TelemetryLevel::Warn, component, "blocked", &reason);
            return Ok(false);
        }

        Ok(true)
    }

    fn log_test_started(&mut self, test_name: &str) {
        self.telemetry.log_test_started(test_name);
        self.remote_panel
            .push_log(TelemetryLevel::Info, test_name, "start", "test started");
    }

    fn log_test_succeeded(&mut self, test_name: &str, detail: &str) {
        self.telemetry.log_test_succeeded(test_name, detail);
        self.remote_panel
            .push_log(TelemetryLevel::Info, test_name, "success", detail);
    }

    fn log_test_failed(&mut self, test_name: &str, fault: &FirmwareFault) {
        self.telemetry.log_test_failed(test_name, fault);
        self.remote_panel.push_log(
            TelemetryLevel::Error,
            test_name,
            "failure",
            &fault.to_string(),
        );
    }

    fn log_heartbeat(&mut self) {
        self.telemetry.log_heartbeat(
            self.state.robot_mode(),
            self.state.harness_state().as_str(),
            self.state.emotion().as_str(),
        );
        self.remote_panel.push_log(
            TelemetryLevel::Info,
            "app",
            "heartbeat",
            &format!(
                "mode={} state={} emotion={}",
                self.state.robot_mode().as_str(),
                self.state.harness_state().as_str(),
                self.state.emotion().as_str(),
            ),
        );
    }

    fn log_fault(&mut self, fault: &FirmwareFault) {
        self.telemetry.log_fault(fault);
        self.remote_panel
            .push_log(TelemetryLevel::Error, "fault", "active", &fault.to_string());
    }
}

pub fn run_fault_loop(fault: FirmwareFault) -> ! {
    let mut telemetry = SerialTelemetry::new();
    telemetry.log_fault(&fault);

    loop {
        telemetry.log_fault(&fault);
        FreeRtos::delay_ms(1_000);
    }
}
