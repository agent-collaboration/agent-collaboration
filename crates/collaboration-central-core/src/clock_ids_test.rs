use super::ClockIds;
use runtime_primitives::{Clock, IdSource, SyncTime};
use std::sync::atomic::{AtomicI64, Ordering};

struct HeldClock(AtomicI64);
impl Clock for &HeldClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

#[test]
fn ids_carry_the_injected_time_and_keep_mint_order_while_the_clock_is_held() {
    let clock = HeldClock(AtomicI64::new(1_700_000_000_123));
    let ids = ClockIds::new(&clock);
    let minted: Vec<_> = (0..64).map(|_| ids.new_id()).collect();
    for id in &minted {
        assert_eq!(id.get_version_num(), 7);
        let (seconds, nanos) = id.get_timestamp().unwrap().to_unix();
        assert_eq!(
            seconds * 1_000 + u64::from(nanos / 1_000_000),
            1_700_000_000_123
        );
    }
    let mut sorted = minted.clone();
    sorted.sort();
    assert_eq!(sorted, minted);
    sorted.dedup();
    assert_eq!(sorted.len(), minted.len());
}

#[test]
fn a_later_clock_reading_mints_a_later_id() {
    let clock = HeldClock(AtomicI64::new(1_700_000_000_000));
    let ids = ClockIds::new(&clock);
    let earlier = ids.new_id();
    clock.0.store(1_700_000_000_001, Ordering::SeqCst);
    assert!(ids.new_id() > earlier);
}
