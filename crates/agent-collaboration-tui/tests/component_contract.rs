//! Public backend and subprocess integration; component-owned cases are colocated under src/.
#[cfg(test)]
#[path = "integration/dispatch_frames_test.rs"]
mod dispatch_frames;
#[cfg(test)]
#[path = "integration/harness_contract_test.rs"]
mod harness_contract;
#[cfg(test)]
#[path = "../src/test_support/integration_support.rs"]
mod test_support;
