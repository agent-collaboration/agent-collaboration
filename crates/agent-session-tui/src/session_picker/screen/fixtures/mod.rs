//! fixtures owns its picker modules and colocated checks.

#[cfg(any(test, feature = "quota-reset-test-harness"))]
pub(crate) mod fixture_data;

#[cfg(all(feature = "quota-reset-test-harness", debug_assertions))]
pub(crate) mod picker_fixture_preferences;

#[cfg(all(feature = "quota-reset-test-harness", debug_assertions))]
pub(crate) mod picker_fixture_receipts;

#[cfg(feature = "quota-reset-test-harness")]
pub(crate) mod picker_fixture_scenarios;

#[cfg(feature = "quota-reset-test-harness")]
pub(crate) mod provider_filter_storyboard_fixture;
