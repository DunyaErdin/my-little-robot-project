use anyhow::Result;

use crate::{platform::pins::AudioOutPins, ports::audio_out::AudioOutPort};

pub struct AudioOutPlaceholderAdapter<I2S> {
    _controller: I2S,
    reservation_summary: String,
}

impl<I2S> AudioOutPlaceholderAdapter<I2S> {
    pub fn new(controller: I2S, pins: AudioOutPins) -> Self {
        Self {
            _controller: controller,
            reservation_summary: pins.summary(),
        }
    }
}

impl<I2S> AudioOutPort for AudioOutPlaceholderAdapter<I2S> {
    fn readiness_note(&self) -> &'static str {
        "I2S speaker scaffold reserved on GPIO14/16/15; playback pipeline not implemented yet"
    }

    fn announce_placeholder_ready(&mut self) -> Result<()> {
        let _ = &self.reservation_summary;
        Ok(())
    }
}
