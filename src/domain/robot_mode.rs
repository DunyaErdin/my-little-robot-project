#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RobotMode {
    Booting,
    Idle,
    Diagnostics,
    Faulted,
}

impl RobotMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Booting => "booting",
            Self::Idle => "idle",
            Self::Diagnostics => "diagnostics",
            Self::Faulted => "faulted",
        }
    }
}
