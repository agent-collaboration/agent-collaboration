// G18: Lead authorized this one test-build exception on 2026-10-12 to observe
// a panic after a known write inside Storage::transaction. Default/production
// builds do not compile this module or the conformance-faults feature.
#[allow(clippy::panic)]
pub(crate) fn panic_inside_transaction() {
    worker::console_log!("G18 deliberate panic after a completed SQL write inside transaction");
    panic!("G18 conformance-only transaction panic");
}
