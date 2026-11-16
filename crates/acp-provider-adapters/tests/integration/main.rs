//! Provider launch contracts and real TypeScript producer-to-decoder integration.

/// The shared owning-check registry (`src/owned_contract_checks.rs`), compiled here from the same
/// source: an integration test cannot turn on its own package's `test-support` feature.
#[path = "../../src/owned_contract_checks.rs"]
mod owned_contract_checks;

mod claude_launch_contract;
mod claude_producer_decoder;
mod model_reasoning_node_tap;
mod owned_contract_checks_real_owners;
mod permission_mode_admission;
