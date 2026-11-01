//! C7 timing rules compare signed intervals separately from receiver-clock skew.
use fleet_identity_protocol::UnixSeconds;

const RECEIVER_CLOCK_ALLOWANCE_SECONDS: i64 = 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceTokenTimeRule {
    Service,
    IssuerDecision,
}

impl ServiceTokenTimeRule {
    const fn maximum_lifetime_seconds(self) -> i64 {
        match self {
            Self::Service => 300,
            Self::IssuerDecision => 60,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ServiceTokenTimeRefusal {
    #[error("the service token's time interval is invalid")]
    InvalidInterval,
    #[error("the service token exceeds its operation's lifetime")]
    LifetimeExceeded,
    #[error("the service token is not yet valid")]
    NotYetValid,
    #[error("the service token has expired")]
    Expired,
}

/// The receiver supplies its injected clock's current seconds. Program Design
/// 0c0ec26d permits skew only across clocks, never inside exp minus iat.
pub fn validate_service_token_times(
    rule: ServiceTokenTimeRule,
    issued_at: UnixSeconds,
    expires_at: UnixSeconds,
    now: UnixSeconds,
) -> Result<(), ServiceTokenTimeRefusal> {
    let lifetime = expires_at
        .0
        .checked_sub(issued_at.0)
        .filter(|lifetime| *lifetime >= 0)
        .ok_or(ServiceTokenTimeRefusal::InvalidInterval)?;
    if lifetime > rule.maximum_lifetime_seconds() {
        return Err(ServiceTokenTimeRefusal::LifetimeExceeded);
    }
    let latest_issue = now
        .0
        .checked_add(RECEIVER_CLOCK_ALLOWANCE_SECONDS)
        .ok_or(ServiceTokenTimeRefusal::InvalidInterval)?;
    if issued_at.0 > latest_issue {
        return Err(ServiceTokenTimeRefusal::NotYetValid);
    }
    let expiry_deadline = expires_at
        .0
        .checked_add(RECEIVER_CLOCK_ALLOWANCE_SECONDS)
        .ok_or(ServiceTokenTimeRefusal::InvalidInterval)?;
    if now.0 >= expiry_deadline {
        return Err(ServiceTokenTimeRefusal::Expired);
    }
    Ok(())
}
