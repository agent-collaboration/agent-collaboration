//! Every timing rule across both daylight-saving transitions of one zone.
//!
//! America/Toronto, 2026: on March 8 local time jumps from 02:00 EST to 03:00 EDT
//! (the spring gap); on November 1 local 01:00-02:00 happens twice, first EDT, then
//! EST (the fall overlap). Elapsed rules count real seconds and never see the zone;
//! calendar rules follow local time.
use agent_automation::{TimingError, TimingRule};
use chrono::{DateTime, Utc};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn instant(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

fn cron(expression: &str) -> TimingRule {
    TimingRule::Cron {
        expression: expression.into(),
        timezone: "America/Toronto".into(),
    }
}

fn require_next(
    rule: &TimingRule,
    anchor: &str,
    after: Option<&str>,
    expected: &str,
    message: &str,
) -> TestResult {
    let after = after.map(instant).transpose()?;
    let next = rule.next_due(instant(anchor)?, after)?;
    if next != Some(instant(expected)?) {
        return Err(format!("{message}: got {next:?}, expected {expected}").into());
    }
    Ok(())
}

#[test]
fn a_one_time_instant_is_exact_inside_both_transitions() -> TestResult {
    // 07:30Z is 03:30 EDT, half an hour after the spring gap.
    // 05:30Z is the first local 01:30 of the fall overlap.
    for at in ["2026-03-08T07:30:00Z", "2026-11-01T05:30:00Z"] {
        require_next(
            &TimingRule::At { at: instant(at)? },
            "2026-01-01T00:00:00Z",
            None,
            at,
            "a one-time instant moved",
        )?;
    }
    Ok(())
}

#[test]
fn after_counts_elapsed_seconds_across_the_spring_gap() -> TestResult {
    // Spring: 01:30 EST (06:30Z) plus one hour is 03:30 EDT, 07:30Z.
    require_next(
        &TimingRule::After { seconds: 3_600 },
        "2026-03-08T06:30:00Z",
        None,
        "2026-03-08T07:30:00Z",
        "one hour after crossed the spring gap by a wall-clock hour",
    )
}

#[test]
fn an_interval_counts_elapsed_seconds_in_the_fall_overlap() -> TestResult {
    // Fall: 00:30 EDT (04:30Z); the second hourly tick lands in the repeated hour.
    require_next(
        &TimingRule::Interval { seconds: 3_600 },
        "2026-11-01T04:30:00Z",
        Some("2026-11-01T05:30:00Z"),
        "2026-11-01T06:30:00Z",
        "an hourly interval skipped or doubled the repeated hour",
    )
}

#[test]
fn a_daily_cron_in_the_spring_gap_fires_once_that_day() -> TestResult {
    // Local 02:30 does not exist on March 8; it fires once, as the gap ends at 03:00 EDT.
    let rule = cron("30 2 * * *");
    require_next(
        &rule,
        "2026-03-08T05:00:00Z",
        None,
        "2026-03-08T07:00:00Z",
        "the skipped 02:30 did not fire as the gap ended",
    )?;
    require_next(
        &rule,
        "2026-03-08T05:00:00Z",
        Some("2026-03-08T07:00:00Z"),
        "2026-03-09T06:30:00Z",
        "the day after the gap is not 02:30 EDT",
    )
}

#[test]
fn a_daily_cron_in_the_fall_overlap_fires_once_at_its_first_occurrence() -> TestResult {
    // Local 01:30 happens at 05:30Z (EDT) and again at 06:30Z (EST).
    let rule = cron("30 1 * * *");
    require_next(
        &rule,
        "2026-11-01T04:00:00Z",
        None,
        "2026-11-01T05:30:00Z",
        "the first 01:30 was not chosen",
    )?;
    require_next(
        &rule,
        "2026-11-01T04:00:00Z",
        Some("2026-11-01T05:30:00Z"),
        "2026-11-02T06:30:00Z",
        "the repeated 01:30 fired twice",
    )
}

#[test]
fn an_hourly_cron_skips_the_spring_gap() -> TestResult {
    let rule = cron("0 * * * *");
    // Spring: 01:00 EST (06:00Z), then 03:00 EDT (07:00Z); there is no 02:00.
    require_next(
        &rule,
        "2026-03-08T05:30:00Z",
        Some("2026-03-08T06:00:00Z"),
        "2026-03-08T07:00:00Z",
        "the hourly cron left a hole after the spring gap",
    )
}

#[test]
fn an_hourly_cron_runs_in_both_halves_of_the_fall_overlap() -> TestResult {
    let rule = cron("0 * * * *");
    // Fall: 01:00 EDT (05:00Z), then 01:00 EST (06:00Z), then 02:00 EST (07:00Z).
    require_next(
        &rule,
        "2026-11-01T04:30:00Z",
        Some("2026-11-01T05:00:00Z"),
        "2026-11-01T06:00:00Z",
        "the hourly cron skipped the repeated 01:00",
    )?;
    require_next(
        &rule,
        "2026-11-01T04:30:00Z",
        Some("2026-11-01T06:00:00Z"),
        "2026-11-01T07:00:00Z",
        "the hourly cron did not reach 02:00 EST",
    )
}

#[test]
fn cron_accepts_exact_iana_names_and_refuses_unknown_zones() -> TestResult {
    let anchor = instant("2026-03-08T00:00:00Z")?;
    for timezone in ["America/Atlantis", "Etc/Unknown", "america/toronto"] {
        let rule = TimingRule::Cron {
            expression: "0 9 * * *".into(),
            timezone: timezone.into(),
        };
        if !matches!(
            rule.next_due(anchor, None),
            Err(TimingError::InvalidTimezone)
        ) {
            return Err(format!("expected invalid timezone for {timezone}").into());
        }
    }
    for (timezone, expected) in [
        ("Etc/UTC", "2026-03-08T09:00:00Z"),
        ("Canada/Eastern", "2026-03-08T13:00:00Z"),
    ] {
        require_next(
            &TimingRule::Cron {
                expression: "0 9 * * *".into(),
                timezone: timezone.into(),
            },
            "2026-03-08T00:00:00Z",
            None,
            expected,
            "a valid IANA alias was not honored",
        )?;
    }
    Ok(())
}

/// Any daily cron, from any instant of 2026 in a zone with both transitions: the
/// next occurrence is later, falls within two days, and lands on the requested
/// local minute, or, for a time the spring gap skips, on the instant the gap ends.
#[hegel::test(test_cases = 200)]
fn any_daily_cron_lands_on_its_local_time_or_the_end_of_the_spring_gap(case: hegel::TestCase) {
    use hegel::generators as gs;
    let hour = case.draw(gs::integers::<u8>().max_value(23));
    let minute = case.draw(gs::integers::<u8>().max_value(59));
    // 2026-01-01T00:00:00Z plus up to a year.
    let start_seconds =
        1_767_225_600 + case.draw(gs::integers::<i64>().min_value(0).max_value(31_536_000));
    let start = DateTime::from_timestamp(start_seconds, 0).expect("2026 instant");
    let rule = cron(&format!("{minute} {hour} * * *"));

    let due = rule
        .next_due(start, None)
        .expect("a daily cron has a next occurrence")
        .expect("a daily cron never ends");

    assert!(due > start, "{due} is not after {start}");
    assert!(
        due - start <= chrono::Duration::days(2),
        "{due} is more than two days after {start}"
    );
    let zone = jiff::tz::TimeZone::get("America/Toronto").expect("zone");
    let local = jiff::Timestamp::from_second(due.timestamp())
        .expect("instant")
        .to_zoned_datetime(zone.clone())
        .datetime();
    let requested = (
        i8::try_from(hour).expect("hour"),
        i8::try_from(minute).expect("minute"),
    );
    if (local.hour(), local.minute()) != requested {
        // Only a skipped local time may move, and only to the end of the gap.
        let skipped = local.date().at(requested.0, requested.1, 0, 0);
        assert!(
            matches!(
                zone.to_ambiguous_zoned_datetime(skipped).offset(),
                jiff::tz::AmbiguousOffset::Gap { .. }
            ),
            "{due} is local {local}, not {requested:?}, and {skipped} is not skipped"
        );
        assert_eq!(
            (local.hour(), local.minute()),
            (3, 0),
            "a skipped time moved to {local}"
        );
    }
}
