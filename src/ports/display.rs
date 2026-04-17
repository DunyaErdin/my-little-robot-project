use anyhow::Result;

pub trait DisplayPort {
    fn show_status(&mut self, title: &str, detail: &str) -> Result<()>;
    fn run_test_frame(&mut self) -> Result<()>;
}
