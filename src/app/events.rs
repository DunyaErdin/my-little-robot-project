use crate::domain::fault::FirmwareFault;

#[derive(Debug, Clone)]
pub enum AppEvent {
    BootCompleted,
    PetTouchTriggered,
    RecordTouchTriggered,
    DisplayTestCompleted,
    MotorTestCompleted,
    AudioInPlaceholderReady,
    AudioOutPlaceholderReady,
    FaultDetected(FirmwareFault),
}
