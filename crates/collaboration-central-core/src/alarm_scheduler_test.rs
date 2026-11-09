use super::next_alarm;
use runtime_primitives::SyncTime;

fn at(seconds: i64) -> SyncTime {
    SyncTime::from_unix_millis(seconds * 1000)
}

#[test]
fn nothing_due_means_no_alarm() {
    assert_eq!(next_alarm(None, Some(at(130))), None);
    assert_eq!(next_alarm(None, None), None);
}

#[test]
fn due_work_inside_the_cadence_waits_for_the_next_batch_slot() {
    // A batch started at 100 s, so the next may start at 130 s.
    assert_eq!(next_alarm(Some(at(105)), Some(at(130))), Some(at(130)));
}

#[test]
fn due_work_after_the_cadence_keeps_its_own_time() {
    assert_eq!(next_alarm(Some(at(200)), Some(at(130))), Some(at(200)));
}

#[test]
fn after_eviction_no_batch_is_remembered_so_due_work_keeps_its_time() {
    // An evicted object lost its cadence memory: at most one extra batch, by design.
    assert_eq!(next_alarm(Some(at(105)), None), Some(at(105)));
}
