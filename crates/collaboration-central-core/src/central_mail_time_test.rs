use agent_automation::TimingRule;
use chrono::{DateTime, Utc};
use workspace_model::Timestamp;

fn timestamp(value: &str) -> Timestamp {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn relative_durations_over_seven_days_report_the_horizon() {
    use workspace_ids::SessionRef;
    use workspace_model::central_mail::{
        CentralMailOperation, CentralMailRefusal, TimingRequirement,
    };

    let now = timestamp("2026-11-14T12:00:00Z");
    let target = SessionRef {
        machine_id: "018f0000-0000-7000-8000-000000000001".parse().unwrap(),
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
