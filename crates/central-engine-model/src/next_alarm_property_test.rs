use super::*;
use hegel::{TestCase, generators as gs};

#[hegel::test]
fn alarm_properties_cover_supplied_times_and_every_pending_combination(case: TestCase) {
    let draw_time = || {
        EngineTime::from_timestamp_millis(
            case.draw(
                gs::integers::<i64>()
                    .min_value(-MAX_ALARM_UNIX_MILLIS)
                    .max_value(MAX_ALARM_UNIX_MILLIS),
            ),
        )
        .unwrap()
    };
    let optional_time = || case.draw(gs::booleans()).then(draw_time);
    let grid = case.draw(gs::booleans()).then(|| {
        let mark = case.draw(
            gs::integers::<i64>()
                .min_value((-MAX_ALARM_UNIX_MILLIS).div_euclid(300_000) + 1)
                .max_value(MAX_ALARM_UNIX_MILLIS.div_euclid(300_000)),
        );
        EngineTime::from_timestamp_millis(mark * 300_000).unwrap()
    });
    let leftovers_beyond_cap = case.draw(gs::booleans());
    let last_batch_start = if leftovers_beyond_cap {
        Some(draw_time())
    } else {
        optional_time()
    };
    let inputs = AlarmInputs {
        now: draw_time(),
        last_batch_start,
        leftovers_beyond_cap,
        earliest_on_grid_due: grid,
        earliest_legacy_due: optional_time(),
    };
    let policy = DueBatchPolicy::DEFAULT;

    let decision = next_alarm(&inputs, &policy);

    assert_eq!(decision, next_alarm(&inputs, &policy));
    let pending =
        grid.is_some() || inputs.earliest_legacy_due.is_some() || inputs.leftovers_beyond_cap;
    assert_eq!(decision == AlarmDecision::Clear, !pending);
    match decision {
        AlarmDecision::Clear => {}
        AlarmDecision::Unpublishable => {
            let no_portable_floor = inputs.last_batch_start.is_some_and(|start| {
                start
                    .checked_add_signed(policy.spacing)
                    .is_none_or(|floor| !supported_time(floor))
            });
            let next_mark_ms = (inputs.now.timestamp_millis().div_euclid(300_000) + 1) * 300_000;
            assert!(
                no_portable_floor || next_mark_ms > MAX_ALARM_UNIX_MILLIS,
                "a valid future mark and portable floor must not be refused"
            );
        }
        AlarmDecision::SetAt(at) => {
            assert!(pending);
            assert!(at > inputs.now);
            assert!(supported_time(at));
            let floor = inputs
                .last_batch_start
                .and_then(|start| start.checked_add_signed(policy.spacing));
            if let Some(floor) = floor {
                assert!(at >= floor);
            }
            assert!(
                at.timestamp_millis().rem_euclid(300_000) == 0
                    || Some(at) == floor
                    || Some(at) == inputs.earliest_legacy_due
            );
        }
    }
}
