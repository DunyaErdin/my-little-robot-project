mod adapters;
mod app;
mod control_panel;
mod domain;
mod platform;
mod ports;

use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::{error, info};

use crate::domain::fault::FirmwareFault;
use crate::platform::board::Board;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("starting ESP32-S3 robot hardware harness");

    let peripherals = match Peripherals::take() {
        Ok(peripherals) => peripherals,
        Err(error) => app::orchestrator::run_fault_loop(FirmwareFault::initialization(format!(
            "failed to acquire ESP-IDF peripherals singleton: {error}"
        ))),
    };

    match Board::from_peripherals(peripherals) {
        Ok(board) => match app::orchestrator::Orchestrator::new(board) {
            Ok(orchestrator) => orchestrator.run(),
            Err(error) => {
                error!("orchestrator initialization failed: {error:#}");
                app::orchestrator::run_fault_loop(FirmwareFault::initialization(error.to_string()))
            }
        },
        Err(error) => {
            error!("board initialization failed: {error:#}");
            app::orchestrator::run_fault_loop(FirmwareFault::initialization(error.to_string()))
        }
    }
}
