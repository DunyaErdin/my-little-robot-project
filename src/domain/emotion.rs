#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RobotEmotion {
    Neutral,
    Focused,
    Curious,
    Alert,
}

impl RobotEmotion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Focused => "focused",
            Self::Curious => "curious",
            Self::Alert => "alert",
        }
    }
}
