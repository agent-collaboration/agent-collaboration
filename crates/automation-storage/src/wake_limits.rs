//! One admission/upgrade policy; schedule timing is deliberately unaffected.
use crate::StorageError;
use agent_automation::TimingRule;
use chrono::{DateTime, Duration, Utc};
use collaboration_protocol::WakeRefusal;

pub(crate) const ONE_SHOT_MAX: u32 = 86_400;
pub(crate) const REPEAT_MAX: u32 = 604_800;
pub(crate) const REPEAT_MIN: u32 = 300;

pub(crate) fn is_repeating(timing: &TimingRule) -> bool {
    matches!(
        timing,
        TimingRule::Interval { .. } | TimingRule::Cron { .. }
    )
}

pub(crate) fn resolve_expiry(
    timing: &TimingRule,
    anchor: DateTime<Utc>,
    expiry: Option<DateTime<Utc>>,
) -> Result<DateTime<Utc>, StorageError> {
    let refuse = |reason| StorageError::WakeRefused(reason);
    if !is_repeating(timing) {
        let latest_due = anchor
            .checked_add_signed(Duration::seconds(i64::from(ONE_SHOT_MAX)))
            .ok_or(StorageError::InvalidRecord)?;
        let due = timing
            .next_due(anchor, None)?
            .ok_or(StorageError::InvalidRecord)?;
        if due > latest_due {
            return Err(refuse(WakeRefusal::TooFarAhead { max: ONE_SHOT_MAX }));
        }
        let cap = due
            .checked_add_signed(Duration::seconds(i64::from(ONE_SHOT_MAX)))
            .ok_or(StorageError::InvalidRecord)?;
        if expiry.is_some_and(|end| end > cap) {
            return Err(refuse(WakeRefusal::LifetimeTooLong { max: ONE_SHOT_MAX }));
        }
        return Ok(expiry.unwrap_or(cap));
    }
    let end = expiry.ok_or_else(|| refuse(WakeRefusal::ExpiryRequired {}))?;
    let cap = anchor
        .checked_add_signed(Duration::seconds(i64::from(REPEAT_MAX)))
        .ok_or(StorageError::InvalidRecord)?;
    if end > cap {
        return Err(refuse(WakeRefusal::LifetimeTooLong { max: REPEAT_MAX }));
    }
    validate_frequency(timing, anchor, end)?;
    Ok(end)
}

pub(crate) fn validate_frequency(
    timing: &TimingRule,
    anchor: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<(), StorageError> {
    let too_short = || StorageError::WakeRefused(WakeRefusal::IntervalTooShort { min: REPEAT_MIN });
    match timing {
        TimingRule::Interval { seconds } if *seconds < REPEAT_MIN => Err(too_short()),
        TimingRule::Cron { .. } => {
            let mut previous = timing.next_due(anchor, None)?;
            while let Some(due) = previous.filter(|at| *at < end) {
                let next = timing.next_due(anchor, Some(due))?;
                if let Some(next) = next.filter(|at| *at < end) {
                    if next <= due {
                        return Err(StorageError::InvalidRecord);
                    }
                    if next - due < Duration::seconds(i64::from(REPEAT_MIN)) {
                        return Err(too_short());
                    }
                }
                previous = next;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
