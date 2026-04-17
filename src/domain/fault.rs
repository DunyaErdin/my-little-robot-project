use core::fmt;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FaultKind {
    Initialization,
    Runtime,
}

impl FaultKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initialization => "initialization",
            Self::Runtime => "runtime",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FirmwareFault {
    kind: FaultKind,
    message: String,
}

impl FirmwareFault {
    pub fn initialization(message: impl Into<String>) -> Self {
        Self {
            kind: FaultKind::Initialization,
            message: message.into(),
        }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self {
            kind: FaultKind::Runtime,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> FaultKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for FirmwareFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "kind={} message={}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for FirmwareFault {}
