use anyhow::{Context, Result};
use esp_idf_hal::{
    i2s::{I2S0, I2S1},
    modem::Modem,
    peripherals::Peripherals,
};

use crate::{
    adapters::{
        AudioInPlaceholderAdapter, AudioOutPlaceholderAdapter, MotorGpioAdapter,
        OledDisplayAdapter, SerialTelemetry, TouchGpioAdapter,
    },
    platform::pins::{
        BoardPins, OLED_I2C_ADDRESS, OLED_I2C_BAUDRATE_HZ, TOUCH_ACTIVE_LEVEL, TOUCH_INPUT_PULL,
    },
};

pub type BoardAudioIn = AudioInPlaceholderAdapter<I2S0<'static>>;
pub type BoardAudioOut = AudioOutPlaceholderAdapter<I2S1<'static>>;

pub struct Board {
    pub display: OledDisplayAdapter,
    pub motion: MotorGpioAdapter,
    pub touch: TouchGpioAdapter,
    pub audio_in: BoardAudioIn,
    pub audio_out: BoardAudioOut,
    pub telemetry: SerialTelemetry,
    pub modem: Modem<'static>,
}

impl Board {
    pub fn from_peripherals(peripherals: Peripherals) -> Result<Self> {
        let Peripherals {
            modem,
            pins,
            i2c0,
            i2s0,
            i2s1,
            ..
        } = peripherals;

        let BoardPins {
            display,
            touch,
            motor,
            audio_in,
            audio_out,
        } = BoardPins::from_hal_pins(pins);

        let motion =
            MotorGpioAdapter::new(motor).context("failed to configure H-bridge GPIO outputs")?;
        let touch = TouchGpioAdapter::new(touch, TOUCH_INPUT_PULL, TOUCH_ACTIVE_LEVEL)
            .context("failed to configure digital touch inputs")?;
        let display =
            OledDisplayAdapter::new(i2c0, display, OLED_I2C_ADDRESS, OLED_I2C_BAUDRATE_HZ)
                .context("failed to configure OLED I2C bus")?;
        let audio_in = AudioInPlaceholderAdapter::new(i2s0, audio_in);
        let audio_out = AudioOutPlaceholderAdapter::new(i2s1, audio_out);
        let telemetry = SerialTelemetry::new();

        Ok(Self {
            display,
            motion,
            touch,
            audio_in,
            audio_out,
            telemetry,
            modem,
        })
    }
}
