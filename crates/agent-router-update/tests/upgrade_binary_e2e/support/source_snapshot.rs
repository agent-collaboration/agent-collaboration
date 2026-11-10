use super::{E2eResult, promote_error};
use std::{path::Path, time::SystemTime};

pub struct SourceSnapshot {
    bytes: Vec<u8>,
    modified: SystemTime,
}

impl SourceSnapshot {
    pub fn capture(path: &Path) -> E2eResult<Self> {
        Ok(Self {
            bytes: std::fs::read(path)?,
            modified: path.metadata()?.modified()?,
        })
    }

    pub fn assert_unchanged(&self, path: &Path) -> E2eResult<()> {
        let current = std::fs::read(path)?;
        if current != self.bytes {
            return Err(promote_error("legacy source bytes changed"));
        }
        if path.metadata()?.modified()? != self.modified {
            return Err(promote_error("legacy source modification time changed"));
        }
        Ok(())
    }
}
