use anyhow::Result;

use crate::{platform::pins::AudioInPins, ports::audio_in::AudioInPort};

pub struct AudioInPlaceholderAdapter<I2S> {
    _controller: I2S,
    reservation_summary: String,
}

impl<I2S> AudioInPlaceholderAdapter<I2S> {
    pub fn new(controller: I2S, pins: AudioInPins) -> Self {
        Self {
            _controller: controller,
            reservation_summary: pins.summary(),
        }
    }
}

impl<I2S> AudioInPort for AudioInPlaceholderAdapter<I2S> {
    fn readiness_note(&self) -> &'static str {
        "I2S microphone scaffold reserved on GPIO12/13/11; capture pipeline not implemented yet"
    }

    fn announce_placeholder_ready(&mut self) -> Result<()> {
        let _ = &self.reservation_summary;
        Ok(())
    }
}
