use anyhow::{Context, Result};
use esp_idf_hal::gpio::{Input, Level, PinDriver, Pull};

use crate::{platform::pins::TouchPins, ports::touch::TouchPort};

/// Temporary digital-input wrapper for breadboard bring-up.
///
/// TODO: Replace this adapter with the ESP32-S3 touch peripheral when true
/// capacitive sensing behavior is required.
pub struct TouchGpioAdapter {
    pet: PinDriver<'static, Input>,
    record: PinDriver<'static, Input>,
    active_level: Level,
}

impl TouchGpioAdapter {
    pub fn new(pins: TouchPins, pull: Pull, active_level: Level) -> Result<Self> {
        let pet = PinDriver::input(pins.pet, pull).context("failed to configure pet touch GPIO")?;
        let record =
            PinDriver::input(pins.record, pull).context("failed to configure record touch GPIO")?;

        Ok(Self {
            pet,
            record,
            active_level,
        })
    }

    fn is_active(driver: &PinDriver<'static, Input>, active_level: Level) -> bool {
        match active_level {
            Level::High => driver.is_high(),
            Level::Low => driver.is_low(),
        }
    }
}

impl TouchPort for TouchGpioAdapter {
    fn pet_triggered(&mut self) -> Result<bool> {
        Ok(Self::is_active(&self.pet, self.active_level))
    }

    fn record_triggered(&mut self) -> Result<bool> {
        Ok(Self::is_active(&self.record, self.active_level))
    }
}
