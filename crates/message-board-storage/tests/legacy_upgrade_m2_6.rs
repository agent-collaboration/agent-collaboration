//! Held legacy-store proof, explicitly bound by the Lead to the redesigned offline upgrade.
// M2.6 is held pending the multi-computer upgrade script (U54). Restore these eight claims
// against that script when it exists; the new Router must never open the legacy BoardStore.
#[cfg(any())]
#[path = "legacy_upgrade_m2_6/mod.rs"]
mod legacy_upgrade_m2_6;
