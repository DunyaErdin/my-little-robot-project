use anyhow::{Context, Result};
use esp_idf_hal::gpio::{Level, Output, PinDriver};

use crate::{platform::pins::MotorPins, ports::motion::MotionPort};

pub struct MotorGpioAdapter {
    in1: PinDriver<'static, Output>,
    in2: PinDriver<'static, Output>,
    in3: PinDriver<'static, Output>,
    in4: PinDriver<'static, Output>,
}

impl MotorGpioAdapter {
    pub fn new(pins: MotorPins) -> Result<Self> {
        let mut adapter = Self {
            in1: PinDriver::output(pins.in1).context("failed to configure motor IN1")?,
            in2: PinDriver::output(pins.in2).context("failed to configure motor IN2")?,
            in3: PinDriver::output(pins.in3).context("failed to configure motor IN3")?,
            in4: PinDriver::output(pins.in4).context("failed to configure motor IN4")?,
        };

        adapter.stop()?;
        Ok(adapter)
    }

    fn apply_pattern(&mut self, in1: Level, in2: Level, in3: Level, in4: Level) -> Result<()> {
        self.in1
            .set_level(in1)
            .context("failed to set motor IN1 level")?;
        self.in2
            .set_level(in2)
            .context("failed to set motor IN2 level")?;
        self.in3
            .set_level(in3)
            .context("failed to set motor IN3 level")?;
        self.in4
            .set_level(in4)
            .context("failed to set motor IN4 level")?;
        Ok(())
    }
}

impl MotionPort for MotorGpioAdapter {
    fn stop(&mut self) -> Result<()> {
        self.apply_pattern(Level::Low, Level::Low, Level::Low, Level::Low)
    }

    fn forward(&mut self) -> Result<()> {
        self.stop()?;
        self.apply_pattern(Level::High, Level::Low, Level::High, Level::Low)
    }

    fn backward(&mut self) -> Result<()> {
        self.stop()?;
        self.apply_pattern(Level::Low, Level::High, Level::Low, Level::High)
    }

    fn turn_left(&mut self) -> Result<()> {
        self.stop()?;
        self.apply_pattern(Level::Low, Level::High, Level::High, Level::Low)
    }

    fn turn_right(&mut self) -> Result<()> {
        self.stop()?;
        self.apply_pattern(Level::High, Level::Low, Level::Low, Level::High)
    }
}
