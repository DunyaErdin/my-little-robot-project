use anyhow::Result;

pub trait MotionPort {
    fn stop(&mut self) -> Result<()>;
    fn forward(&mut self) -> Result<()>;
    fn backward(&mut self) -> Result<()>;
    fn turn_left(&mut self) -> Result<()>;
    fn turn_right(&mut self) -> Result<()>;
}
