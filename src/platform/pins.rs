use core::fmt;

use esp_idf_hal::gpio::{AnyIOPin, AnyInputPin, AnyOutputPin, Level, Pin, Pins, Pull};

pub const OLED_I2C_ADDRESS: u8 = 0x3C;
pub const OLED_I2C_BAUDRATE_HZ: u32 = 400_000;
pub const TOUCH_INPUT_PULL: Pull = Pull::Down;
pub const TOUCH_ACTIVE_LEVEL: Level = Level::High;

#[derive(Debug, Clone, Copy)]
pub struct BoardPinMap {
    pub oled_scl: u8,
    pub oled_sda: u8,
    pub pet_touch: u8,
    pub record_touch: u8,
    pub mic_sck: u8,
    pub mic_ws: u8,
    pub mic_sd: u8,
    pub motor_in1: u8,
    pub motor_in2: u8,
    pub motor_in3: u8,
    pub motor_in4: u8,
    pub speaker_lrc: u8,
    pub speaker_bclk: u8,
    pub speaker_din: u8,
}

pub const PIN_MAP: BoardPinMap = BoardPinMap {
    oled_scl: 9,
    oled_sda: 8,
    pet_touch: 5,
    record_touch: 6,
    mic_sck: 12,
    mic_ws: 13,
    mic_sd: 11,
    motor_in1: 1,
    motor_in2: 2,
    motor_in3: 41,
    motor_in4: 42,
    speaker_lrc: 14,
    speaker_bclk: 16,
    speaker_din: 15,
};

impl fmt::Display for BoardPinMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "oled[scl=GPIO{} sda=GPIO{}] touch[pet=GPIO{} record=GPIO{}] mic[sck=GPIO{} ws=GPIO{} sd=GPIO{}] motor[in1=GPIO{} in2=GPIO{} in3=GPIO{} in4=GPIO{}] speaker[lrc=GPIO{} bclk=GPIO{} din=GPIO{}]",
            self.oled_scl,
            self.oled_sda,
            self.pet_touch,
            self.record_touch,
            self.mic_sck,
            self.mic_ws,
            self.mic_sd,
            self.motor_in1,
            self.motor_in2,
            self.motor_in3,
            self.motor_in4,
            self.speaker_lrc,
            self.speaker_bclk,
            self.speaker_din,
        )
    }
}

pub struct DisplayPins {
    pub scl: AnyIOPin<'static>,
    pub sda: AnyIOPin<'static>,
}

pub struct TouchPins {
    pub pet: AnyInputPin<'static>,
    pub record: AnyInputPin<'static>,
}

pub struct MotorPins {
    pub in1: AnyOutputPin<'static>,
    pub in2: AnyOutputPin<'static>,
    pub in3: AnyOutputPin<'static>,
    pub in4: AnyOutputPin<'static>,
}

pub struct AudioInPins {
    pub sck: AnyIOPin<'static>,
    pub ws: AnyIOPin<'static>,
    pub sd: AnyInputPin<'static>,
}

impl AudioInPins {
    pub fn summary(&self) -> String {
        format!(
            "sck=GPIO{} ws=GPIO{} sd=GPIO{}",
            self.sck.pin(),
            self.ws.pin(),
            self.sd.pin(),
        )
    }
}

pub struct AudioOutPins {
    pub lrc: AnyIOPin<'static>,
    pub bclk: AnyIOPin<'static>,
    pub din: AnyOutputPin<'static>,
}

impl AudioOutPins {
    pub fn summary(&self) -> String {
        format!(
            "lrc=GPIO{} bclk=GPIO{} din=GPIO{}",
            self.lrc.pin(),
            self.bclk.pin(),
            self.din.pin(),
        )
    }
}

pub struct BoardPins {
    pub display: DisplayPins,
    pub touch: TouchPins,
    pub motor: MotorPins,
    pub audio_in: AudioInPins,
    pub audio_out: AudioOutPins,
}

impl BoardPins {
    pub fn from_hal_pins(pins: Pins) -> Self {
        Self {
            display: DisplayPins {
                scl: pins.gpio9.degrade_input_output(),
                sda: pins.gpio8.degrade_input_output(),
            },
            touch: TouchPins {
                pet: pins.gpio5.degrade_input(),
                record: pins.gpio6.degrade_input(),
            },
            motor: MotorPins {
                in1: pins.gpio1.degrade_output(),
                in2: pins.gpio2.degrade_output(),
                in3: pins.gpio41.degrade_output(),
                in4: pins.gpio42.degrade_output(),
            },
            audio_in: AudioInPins {
                sck: pins.gpio12.degrade_input_output(),
                ws: pins.gpio13.degrade_input_output(),
                sd: pins.gpio11.degrade_input(),
            },
            audio_out: AudioOutPins {
                lrc: pins.gpio14.degrade_input_output(),
                bclk: pins.gpio16.degrade_input_output(),
                din: pins.gpio15.degrade_output(),
            },
        }
    }
}
