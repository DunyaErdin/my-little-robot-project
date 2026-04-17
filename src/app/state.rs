use std::time::{Duration, Instant};

use crate::domain::{emotion::RobotEmotion, fault::FirmwareFault, robot_mode::RobotMode};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
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

impl HarnessState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Idle => "idle",
            Self::TestTouch => "test_touch",
            Self::TestMotor => "test_motor",
            Self::TestDisplay => "test_display",
            Self::TestAudioInPlaceholder => "test_audio_in_placeholder",
            Self::TestAudioOutPlaceholder => "test_audio_out_placeholder",
            Self::Fault => "fault",
        }
    }
}

pub struct AppState {
    harness_state: HarnessState,
    robot_mode: RobotMode,
    emotion: RobotEmotion,
    last_heartbeat_at: Instant,
    last_fault: Option<FirmwareFault>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            harness_state: HarnessState::Boot,
            robot_mode: RobotMode::Booting,
            emotion: RobotEmotion::Focused,
            last_heartbeat_at: Instant::now(),
            last_fault: None,
        }
    }

    pub fn harness_state(&self) -> HarnessState {
        self.harness_state
    }

    pub fn robot_mode(&self) -> RobotMode {
        self.robot_mode
    }

    pub fn emotion(&self) -> RobotEmotion {
        self.emotion
    }

    pub fn last_fault(&self) -> Option<&FirmwareFault> {
        self.last_fault.as_ref()
    }

    pub fn transition_to(&mut self, next: HarnessState) {
        self.harness_state = next;
        self.robot_mode = match next {
            HarnessState::Boot => RobotMode::Booting,
            HarnessState::Idle => RobotMode::Idle,
            HarnessState::Fault => RobotMode::Faulted,
            HarnessState::TestTouch
            | HarnessState::TestMotor
            | HarnessState::TestDisplay
            | HarnessState::TestAudioInPlaceholder
            | HarnessState::TestAudioOutPlaceholder => RobotMode::Diagnostics,
        };
        self.emotion = match next {
            HarnessState::Boot => RobotEmotion::Focused,
            HarnessState::Idle => RobotEmotion::Neutral,
            HarnessState::TestTouch => RobotEmotion::Curious,
            HarnessState::TestMotor
            | HarnessState::TestDisplay
            | HarnessState::TestAudioInPlaceholder
            | HarnessState::TestAudioOutPlaceholder => RobotEmotion::Focused,
            HarnessState::Fault => RobotEmotion::Alert,
        };
    }

    pub fn heartbeat_due(&self, interval: Duration) -> bool {
        self.last_heartbeat_at.elapsed() >= interval
    }

    pub fn mark_heartbeat(&mut self) {
        self.last_heartbeat_at = Instant::now();
    }

    pub fn set_fault(&mut self, fault: FirmwareFault) {
        self.last_fault = Some(fault);
        self.transition_to(HarnessState::Fault);
    }
}
