use agent_session_inventory::picker_data_query::SessionsPickerRoot;
use std::path::PathBuf;

use agent_session_inventory::sessions::RepositoryIdentity;
use agent_session_inventory::sessions::SessionPickerRecord;
use agent_session_inventory::sessions::SessionsProvider;
use agent_session_inventory::sessions::SessionsSort;
use agent_session_inventory::sessions::SessionsSource;

/// Request needed to render and drive the interactive sessions picker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionsPickerRequest {
    pub root: SessionsPickerRoot,
    pub provider: SessionsProvider,
    pub source: SessionsSource,
    pub sort: SessionsSort,
    pub current_dir: PathBuf,
    pub repository_identity: RepositoryIdentity,
    pub current_provider: Option<String>,
    pub new_session_args_display: String,
    pub native_arguments: Vec<std::ffi::OsString>,
    pub include_empty_sessions: bool,
    pub records: Vec<SessionPickerRecord>,
    pub router_registry: agent_session_inventory::sessions::RouterRegistryRead,
    pub machine_mode: super::PickerMachineSourceMode,
}

pub type SessionsPickerRecordLoader =
    agent_session_inventory::source_inventory_request::SourceInventoryLoader;

impl Default for SessionsPickerRequest {
    fn default() -> Self {
        Self {
            root: SessionsPickerRoot::Cwd,
            provider: SessionsProvider::Any,
            source: SessionsSource::Interactive,
            sort: SessionsSort::Updated,
            current_dir: PathBuf::new(),
            repository_identity: RepositoryIdentity {
                normalized_origin: None,
                live_roots: Vec::new(),
                repository_basename: String::new(),
                fallback_cwd: None,
            },
            current_provider: None,
            new_session_args_display: String::new(),
            native_arguments: Vec::new(),
            include_empty_sessions: false,
            records: Vec::new(),
            router_registry: agent_session_inventory::sessions::RouterRegistryRead::Missing,
            machine_mode: super::PickerMachineSourceMode::HostedDefault,
        }
    }
}
