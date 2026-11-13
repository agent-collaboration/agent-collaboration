//! One-time Router root updates, separate from the running Router.
mod dead_files_step;
mod ledger_step;
mod merge_step;
mod proxy_store_step;
mod rename_step;
mod reset_step;
mod root_layout;
mod router_update;
mod stale_files;
pub use proxy_store_step::ProxyStoreChange;
mod settings_step;
pub use root_layout::RouterUpdateError;
pub use router_update::{
    PendingUpdateStep, PlannedChange, RouterUpdatePlan, plan_router_update, run_router_update,
};
pub use settings_step::{MachineListChange, SettingsChange, SettingsFileChange};
pub use stale_files::StaleFile;
mod update_barrier;
