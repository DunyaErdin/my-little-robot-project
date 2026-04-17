pub mod audio_in_placeholder;
pub mod audio_out_placeholder;
pub mod motor_gpio;
pub mod oled_display;
pub mod serial_telemetry;
pub mod touch_gpio;

pub use audio_in_placeholder::AudioInPlaceholderAdapter;
pub use audio_out_placeholder::AudioOutPlaceholderAdapter;
pub use motor_gpio::MotorGpioAdapter;
pub use oled_display::OledDisplayAdapter;
pub use serial_telemetry::SerialTelemetry;
pub use touch_gpio::TouchGpioAdapter;
