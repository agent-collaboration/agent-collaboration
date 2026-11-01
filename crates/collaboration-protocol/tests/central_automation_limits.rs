use collaboration_protocol::TimingRequest;
use collaboration_protocol::{
    AutomationKind, AutomationTiming, AutomationValidationError, GridCron, GridPeriod, GridTime,
};
use schemars::schema_for;
use serde_json::json;

fn millis(value: &str) -> Result<i64, chrono::ParseError> {
    value
        .parse::<chrono::DateTime<chrono::Utc>>()
        .map(|timestamp| timestamp.timestamp_millis())
}

#[test]
fn automation_timing_schema_refuses_relative_after() {
    let schema =
        serde_json::to_value(schema_for!(collaboration_protocol::WakeSendRequest)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut request = json!({"operationId":"019f0000-0000-7000-8000-000000000001",
        "requestedBy":{"kind":"owner"},"replace":false,
        "message":{"target":{"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000002","endpointId":"claude-local"},"sessionId":"fixture"},"content":{"kind":"router","text":"fixture"},"delivery":"auto"},
        "timing":{"kind":"at", "at":"2026-10-31T10:05:00Z"},"expiry":{"kind":"none"}});
    assert!(validator.is_valid(&request));
    request["timing"] = json!({"kind":"after","seconds":300});
    assert!(
        !validator.is_valid(&request),
        "R6g refuses every relative after, even a five-minute duration"
    );
}

#[test]
fn historical_timing_schema_still_describes_pre_move_records() {
    let schema = serde_json::to_value(schema_for!(TimingRequest)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let historical = json!({"kind":"after","seconds":60});
    assert!(validator.is_valid(&historical));
    assert!(serde_json::from_value::<TimingRequest>(historical).is_ok());
}

#[test]
fn grid_constructor_refuses_instead_of_rounding_and_names_literal_neighbours() {
    let now = millis("2026-10-31T10:00:00Z").unwrap();
    for (value, expected) in [
        (
            json!({"kind":"at","at":"2026-10-31T10:02:00Z"}),
            ("2026-10-31T10:00:00Z", "2026-10-31T10:05:00Z"),
        ),
        (
            json!({"kind":"interval","seconds":420}),
            ("2026-10-31T10:05:00Z", "2026-10-31T10:10:00Z"),
        ),
        (
            json!({"kind":"after","seconds":120}),
            ("2026-10-31T10:00:00Z", "2026-10-31T10:05:00Z"),
        ),
    ] {
        let request: TimingRequest = serde_json::from_value(value).unwrap();
        let error = AutomationTiming::resolve(&request, AutomationKind::Schedule, now).unwrap_err();
        let AutomationValidationError::OffGrid { earlier, later, .. } = error else {
            panic!("R6g requires offGrid");
        };
        assert_eq!((earlier.as_str(), later.as_str()), expected);
    }
    for at in ["2026-10-31T10:05:00.001Z", "2026-10-31T10:05:00.000000001Z"] {
        let request: TimingRequest = serde_json::from_value(json!({"kind":"at","at":at})).unwrap();
        assert!(AutomationTiming::resolve(&request, AutomationKind::Schedule, now).is_err());
    }
}

#[test]
fn omitted_interval_start_uses_strictly_next_grid_boundary_and_is_stored() {
    let request: TimingRequest =
        serde_json::from_value(json!({"kind":"interval","seconds":600})).unwrap();
    for (now, chosen) in [
        ("2026-10-31T10:04:59.999Z", "2026-10-31T10:05:00Z"),
        ("2026-10-31T10:05:00Z", "2026-10-31T10:10:00Z"),
    ] {
        let resolved =
            AutomationTiming::resolve(&request, AutomationKind::Wake, millis(now).unwrap())
                .unwrap();
        assert_eq!(resolved.first_due().unix_millis(), millis(chosen).unwrap());
        let stored = serde_json::to_vec(&resolved).unwrap();
        let reopened: AutomationTiming = serde_json::from_slice(&stored).unwrap();
        assert_eq!(reopened.first_due().unix_millis(), millis(chosen).unwrap());
    }
    assert!(GridPeriod::for_wake(300).is_err());
    assert!(GridPeriod::for_wake(600).is_ok());
    let cron: TimingRequest =
        serde_json::from_value(json!({"kind":"cron","expression":"*/5 * * * *","timezone":"UTC"}))
            .unwrap();
    assert!(
        AutomationTiming::resolve(
            &cron,
            AutomationKind::Wake,
            millis("2026-10-31T10:00:00Z").unwrap()
        )
        .is_err()
    );
    assert!(
        AutomationTiming::resolve(
            &cron,
            AutomationKind::Schedule,
            millis("2026-10-31T10:00:00Z").unwrap()
        )
        .is_ok()
    );
}

#[test]
fn caller_can_choose_the_first_interval_deadline_and_off_grid_is_refused() {
    let now = millis("2026-10-31T10:00:00Z").unwrap();
    let request = json!({"kind":"interval","seconds":600,"firstDue":"2026-10-31T10:15:00Z"});
    let timing: TimingRequest = serde_json::from_value(request.clone()).unwrap();
    assert_eq!(serde_json::to_value(&timing).unwrap(), request);
    let resolved = AutomationTiming::resolve(&timing, AutomationKind::Wake, now).unwrap();
    assert_eq!(
        resolved.first_due().unix_millis(),
        millis("2026-10-31T10:15:00Z").unwrap()
    );
    assert_eq!(
        resolved
            .next_after(millis("2026-10-31T10:15:00Z").unwrap())
            .unwrap()
            .unwrap()
            .unix_millis(),
        millis("2026-10-31T10:25:00Z").unwrap()
    );

    let off_grid: TimingRequest = serde_json::from_value(
        json!({"kind":"interval","seconds":600,"firstDue":"2026-10-31T10:12:00Z"}),
    )
    .unwrap();
    let error = AutomationTiming::resolve(&off_grid, AutomationKind::Wake, now).unwrap_err();
    let AutomationValidationError::OffGrid { earlier, later, .. } = error else {
        panic!("expected offGrid");
    };
    assert_eq!(earlier, "2026-10-31T10:10:00Z");
    assert_eq!(later, "2026-10-31T10:15:00Z");
}

#[test]
fn whole_cron_expansion_is_checked_and_dst_keeps_utc_grid() {
    for expression in ["3 * * * *", "0,3 * * * *", "0-59/7 * * * *", "*/1 * * * *"] {
        assert!(matches!(
            GridCron::new(expression.into(), "UTC".into()),
            Err(AutomationValidationError::OffGrid { .. })
        ));
    }
    for expression in ["*/5 * * * *", "0,15,30,45 * * * *", "0-55/5 * * * *"] {
        let cron = GridCron::new(expression.into(), "America/Toronto".into()).unwrap();
        for date in ["2026-03-08T06:58:00Z", "2026-11-01T05:58:00Z"] {
            let next = cron.next_after(millis(date).unwrap()).unwrap();
            assert_eq!(next.unix_millis() % 300_000, 0);
            assert!(next.unix_millis() > millis(date).unwrap());
        }
    }
    let historical = GridCron::new("*/5 * * * *".into(), "Africa/Monrovia".into()).unwrap();
    assert!(matches!(
        historical.next_after(millis("1960-01-01T10:00:00Z").unwrap()),
        Err(AutomationValidationError::OffGrid { .. })
    ));
}

#[test]
fn stored_grid_values_are_validated_on_decode() {
    assert!(serde_json::from_value::<AutomationTiming>(json!({"kind":"cron", "cron":{"expression":"0 * * * *","timezone":"UTC"},"firstDue":300000})).is_err());
    assert!(serde_json::from_str::<GridTime>("120000").is_err());
    assert!(serde_json::from_str::<GridPeriod>("420").is_err());
    assert!(
        serde_json::from_value::<GridCron>(json!({"expression":"0,3 * * * *","timezone":"UTC"}))
            .is_err()
    );
    assert!(
        serde_json::from_str::<GridPeriod>("0")
            .unwrap_err()
            .to_string()
            .contains("limitExceeded")
    );
}

proptest::proptest! {
    #[test]
    fn grid_membership_and_period_boundaries_are_exact(step in -1_000_000i64..1_000_000, offset in 0i64..300_000) {
        let value = step * 300_000 + offset;
        proptest::prop_assert_eq!(GridTime::new(value).is_ok(), offset == 0);
    }
}
