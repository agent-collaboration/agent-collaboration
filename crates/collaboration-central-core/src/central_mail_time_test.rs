use super::{aligned_at_or_after, first_due, lifetime_end, on_grid, valid_rule};
use agent_automation::TimingRule;
use chrono::{DateTime, Utc};
use workspace_model::Timestamp;

fn timestamp(value: &str) -> Timestamp {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn grid_alignment_includes_fractional_and_negative_timestamps() {
    let mark = timestamp("2026-11-14T12:05:00Z");
    assert!(on_grid(mark));
    assert_eq!(aligned_at_or_after(mark, 300), Some(mark));
    let fractional = timestamp("2026-11-14T12:05:00.000000001Z");
    assert!(!on_grid(fractional));
    assert_eq!(
        aligned_at_or_after(fractional, 300),
        Some(timestamp("2026-11-14T12:10:00Z"))
    );
    assert_eq!(
        aligned_at_or_after(timestamp("1969-12-31T23:59:59.5Z"), 300),
        Some(timestamp("1970-01-01T00:00:00Z"))
    );
}

#[test]
fn relative_first_due_uses_the_aligned_anchor() {
    let now = timestamp("2026-11-14T12:02:00Z");
    let interval = TimingRule::Interval { seconds: 300 };
    let after = TimingRule::After { seconds: 300 };
    assert_eq!(
        first_due(&interval, now).unwrap(),
        Some(timestamp("2026-11-14T12:05:00Z"))
    );
    assert_eq!(
        first_due(&after, now).unwrap(),
        Some(timestamp("2026-11-14T12:10:00Z"))
    );
    assert_eq!(lifetime_end(now), Some(timestamp("2026-11-21T12:02:00Z")));
}

#[test]
fn cron_checks_every_selected_minute_and_utc_occurrences() {
    let now = timestamp("2026-11-14T12:02:00Z");
    let expiry = timestamp("2026-11-21T12:02:00Z");
    let good = TimingRule::Cron {
        expression: "0,5,10-20/5 * * * *".into(),
        timezone: "UTC".into(),
    };
    let bad = TimingRule::Cron {
        expression: "0,5,11 * * * *".into(),
        timezone: "UTC".into(),
    };
    assert!(valid_rule(&good, now, expiry).unwrap());
    assert!(!valid_rule(&bad, now, expiry).unwrap());
    let nepal = TimingRule::Cron {
        expression: "*/5 * * * *".into(),
        timezone: "Asia/Kathmandu".into(),
    };
    assert!(valid_rule(&nepal, now, expiry).unwrap());
}

#[test]
fn relative_durations_over_seven_days_report_the_horizon() {
    use collaboration_protocol::central_mail::{
        CentralMailOperation, CentralMailRefusal, TimingRequirement,
    };
    use workspace_model::SessionAddress;

    let now = timestamp("2026-11-14T12:00:00Z");
    let target = SessionAddress {
        machine_id: "018f0000-0000-7000-8000-000000000001".parse().unwrap(),
        endpoint: "codex-local".to_owned().try_into().unwrap(),
        session_id: "test-session".to_owned().try_into().unwrap(),
    };
    for timing in [
        TimingRule::After { seconds: 604_801 },
        TimingRule::Interval { seconds: 604_801 },
        TimingRule::After {
            seconds: 31_536_001,
        },
        TimingRule::Interval {
            seconds: 31_536_001,
        },
    ] {
        let operation = CentralMailOperation::SetAutomation {
            target: target.clone(),
            timing,
            text: "test".to_owned().try_into().unwrap(),
        };
        assert_eq!(
            super::refusal(&operation, now),
            Some(CentralMailRefusal::TimingViolation {
                field: "timing.seconds".to_owned(),
                requirement: TimingRequirement::WithinSevenDays,
            })
        );
    }
    for timing in [
        TimingRule::After { seconds: 604_800 },
        TimingRule::Interval { seconds: 604_800 },
    ] {
        let operation = CentralMailOperation::SetAutomation {
            target: target.clone(),
            timing,
            text: "test".to_owned().try_into().unwrap(),
        };
        assert_eq!(super::refusal(&operation, now), None);
    }
}
