use super::*;
use hegel::{TestCase, generators as gs};

const MARK: i64 = 1_763_380_800_000; // 2025-11-17T12:00:00Z, on a minute mark

fn at(millis: i64) -> SyncTime {
    SyncTime::from_unix_millis(millis)
}

#[test]
fn a_write_stored_before_the_window_gets_the_coming_mark() {
    assert_eq!(earliest_open_mark(at(MARK - 10_000), None), at(MARK));
}

#[test]
fn a_write_stored_inside_an_unflushed_window_still_makes_that_mark() {
    assert_eq!(earliest_open_mark(at(MARK + 2_000), None), at(MARK));
    assert_eq!(earliest_open_mark(at(MARK - 2_000), None), at(MARK));
}

#[test]
fn a_write_stored_after_its_mark_flushed_waits_for_the_next() {
    assert_eq!(
        earliest_open_mark(at(MARK - 1_000), Some(at(MARK))),
        at(MARK + FLUSH_INTERVAL_MILLIS)
    );
}

#[test]
fn the_window_is_three_seconds_either_side() {
    assert_eq!(mark_in_window(at(MARK - 3_000)), Some(at(MARK)));
    assert_eq!(mark_in_window(at(MARK + 3_000)), Some(at(MARK)));
    assert_eq!(mark_in_window(at(MARK - 3_001)), None);
    assert_eq!(mark_in_window(at(MARK + 3_001)), None);
}

#[hegel::test]
fn the_open_mark_is_a_future_or_current_window_mark_after_the_last_flush(case: TestCase) {
    let now = at(case.draw(gs::integers::<i64>().min_value(0).max_value(1 << 45)));
    let last_flushed = case.draw(gs::booleans()).then(|| {
        let index = case.draw(
            gs::integers::<i64>()
                .min_value(0)
                .max_value((1 << 45) / 60_000),
        );
        at(index * FLUSH_INTERVAL_MILLIS)
    });

    let mark = earliest_open_mark(now, last_flushed);

    assert_eq!(mark.unix_millis().rem_euclid(FLUSH_INTERVAL_MILLIS), 0);
    assert!(mark.unix_millis() + FLUSH_JITTER_MILLIS >= now.unix_millis());
    if let Some(flushed) = last_flushed {
        assert!(mark > flushed);
    }
    let earlier = at(mark.unix_millis() - FLUSH_INTERVAL_MILLIS);
    let earlier_open = earlier.unix_millis() + FLUSH_JITTER_MILLIS >= now.unix_millis()
        && last_flushed.is_none_or(|flushed| earlier > flushed);
    assert!(!earlier_open, "an earlier mark was still open");
}
