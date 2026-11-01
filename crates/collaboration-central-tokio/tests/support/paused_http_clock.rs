#![cfg(test)]
//! Reads only the paused Tokio clock for per-case before/after assertions.
//! Call exclusively inside `#[tokio::test(start_paused = true)]` runtimes.
//! This never caps a wait, advances time, or reads the standard clock.

pub struct PausedHttpClock {
    captured_at: tokio::time::Instant,
}

#[derive(Debug, thiserror::Error)]
pub enum UnexpectedClockAdvance {
    #[error(
        "{case_name}: paused HTTP clock advanced by {advanced_by:?} without an explicit instruction"
    )]
    Advanced {
        case_name: String,
        advanced_by: std::time::Duration,
    },
    #[error("{case_name}: paused HTTP clock moved backwards without an explicit instruction")]
    MovedBackwards { case_name: String },
}

impl PausedHttpClock {
    pub fn before_case() -> Self {
        Self {
            captured_at: tokio::time::Instant::now(),
        }
    }

    pub fn require_no_advance(self, case_name: &str) -> Result<(), UnexpectedClockAdvance> {
        let after = tokio::time::Instant::now();
        if after == self.captured_at {
            return Ok(());
        }
        Err(match after.checked_duration_since(self.captured_at) {
            Some(advanced_by) => UnexpectedClockAdvance::Advanced {
                case_name: case_name.to_owned(),
                advanced_by,
            },
            None => UnexpectedClockAdvance::MovedBackwards {
                case_name: case_name.to_owned(),
            },
        })
    }
}
