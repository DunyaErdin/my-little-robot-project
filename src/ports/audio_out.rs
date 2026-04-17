use anyhow::Result;

pub trait AudioOutPort {
    fn readiness_note(&self) -> &'static str;
    fn announce_placeholder_ready(&mut self) -> Result<()>;
}
