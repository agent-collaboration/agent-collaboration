#![deny(clippy::too_many_lines)]

//! Original test roots retain their case namespaces.

#[path = "../support/upload_support/central_fixture.rs"]
mod central_fixture;
#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
#[path = "../support/mod.rs"]
mod fixture_support;
#[expect(
    dead_code,
    reason = "Upload helpers are shared with sibling integration targets."
)]
#[path = "../support/upload_support/mod.rs"]
mod upload_support;

#[path = "upgrade_binary_e2e/support/mod.rs"]
mod e2e_support;

#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
#[path = "../../examples/legacy_selection_oracle/oracle_errors.rs"]
mod oracle_errors;
#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
#[path = "../../examples/legacy_selection_oracle/oracle_hashes.rs"]
mod oracle_hashes;
#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
#[path = "../../examples/legacy_selection_oracle/post_selection.rs"]
mod post_selection;
#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
#[path = "../../examples/legacy_selection_oracle/sealed_source.rs"]
mod sealed_source;

mod upgrade_binary_e2e;
