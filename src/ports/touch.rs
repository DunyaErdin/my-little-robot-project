use anyhow::Result;

#[derive(Debug, Default, Clone, Copy)]
pub struct TouchSnapshot {
    pub pet: bool,
    pub record: bool,
}

impl TouchSnapshot {
    pub const fn any_triggered(self) -> bool {
        self.pet || self.record
    }

    pub fn describe(self) -> String {
        format!("pet={} record={}", self.pet, self.record)
    }
}

pub trait TouchPort {
    fn pet_triggered(&mut self) -> Result<bool>;
    fn record_triggered(&mut self) -> Result<bool>;

    fn read_snapshot(&mut self) -> Result<TouchSnapshot> {
        Ok(TouchSnapshot {
            pet: self.pet_triggered()?,
            record: self.record_triggered()?,
        })
    }
}
