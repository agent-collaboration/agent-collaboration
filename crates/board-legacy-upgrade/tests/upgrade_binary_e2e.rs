#![deny(clippy::too_many_lines)]

#[path = "upload_support/central_fixture.rs"]
mod central_fixture;
#[path = "upgrade_binary_e2e/support/mod.rs"]
mod e2e_support;
#[path = "support/mod.rs"]
mod fixture_support;
#[expect(
    dead_code,
    reason = "Upload helpers are shared with sibling integration targets."
)]
#[path = "upload_support/mod.rs"]
mod upload_support;

#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
#[path = "../examples/legacy_selection_oracle/oracle_errors.rs"]
mod oracle_errors;
#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
#[path = "../examples/legacy_selection_oracle/oracle_hashes.rs"]
mod oracle_hashes;
#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
#[path = "../examples/legacy_selection_oracle/post_selection.rs"]
mod post_selection;
#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
#[path = "../examples/legacy_selection_oracle/sealed_source.rs"]
mod sealed_source;

#[path = "upgrade_binary_e2e/ib01_preparation.rs"]
mod ib01_preparation;
#[path = "upgrade_binary_e2e/ib01_publication.rs"]
mod ib01_publication;
#[path = "upgrade_binary_e2e/ib02_replay.rs"]
mod ib02_replay;
#[path = "upgrade_binary_e2e/ib03_prepublication_restart.rs"]
mod ib03_prepublication_restart;
#[path = "upgrade_binary_e2e/ib04_postpublication_restart.rs"]
mod ib04_postpublication_restart;
