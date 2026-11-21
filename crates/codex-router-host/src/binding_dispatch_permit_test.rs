use super::*;
use futures_util::FutureExt as _;
use hegel::generators as gs;

#[hegel::test(seed = Some(2911802), test_cases = 100)]
fn release_before_the_queue_waiter_polls_still_wakes_it(case: hegel::TestCase) {
    let binding = BindingUncertainty::default();
    let mut watcher = binding.subscribe_eligibility();
    let before = *watcher.borrow_and_update();
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(16)) {
        binding.eligibility_changed();
    }
    assert!(matches!(watcher.changed().now_or_never(), Some(Ok(()))));
    assert_ne!(*watcher.borrow_and_update(), before);
}
