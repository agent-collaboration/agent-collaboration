//! What a session's start or restore tells its agent about the session's
//! folders, and what it keeps for the in-folder rule (D1 R18). A create and a
//! load or resume build these the same way, from the session's scratch folder
//! and work roots.

use crate::in_folder_allowance::{InFolderGrant, session_folders};
use agent_client_protocol::schema::v1::Meta;
use std::path::{Path, PathBuf};

pub(crate) struct SessionStartFolders {
    /// The scratch folder, then the work roots that exist; empty for an agent
    /// that doesn't accept additional directories.
    pub(crate) additional_directories: Vec<PathBuf>,
    /// The provider's own request metadata for these folders (Claude's
    /// work-root layer and hook).
    pub(crate) meta: Option<Meta>,
    /// The session's in-folder roots; `None` without a scratch folder or when
    /// a folder can't be resolved, so the session asks for everything.
    pub(crate) in_folder: Option<InFolderGrant>,
}

pub(crate) fn session_start_folders(
    cwd: &Path,
    scratch_folder: Option<&Path>,
    work_roots: &crate::SessionWorkRoots,
    behavior: &dyn crate::ProviderBehavior,
    supports_additional_directories: bool,
) -> SessionStartFolders {
    let folders = session_folders(scratch_folder, &work_roots.roots);
    let meta = behavior.session_new_meta(&crate::SessionWorkRoots {
        roots: folders.clone(),
        ask_outside_command: work_roots.ask_outside_command.clone(),
        router_auto: work_roots.router_auto,
    });
    let in_folder = scratch_folder
        .and_then(|scratch_folder| InFolderGrant::new(cwd, scratch_folder, &work_roots.roots));
    SessionStartFolders {
        additional_directories: if supports_additional_directories {
            folders
        } else {
            Vec::new()
        },
        meta,
        in_folder,
    }
}

/// Grants a registered restore its in-folder roots, active only when the mode
/// the agent restored it in is Router auto as its create asked (D1 R18; owner
/// Decision 3), and answers the session's mode report. A session without
/// roots (no scratch folder recorded) gets no grant and asks for everything.
pub(crate) fn grant_restored_session(
    grants: &crate::in_folder_allowance::InFolderGrants,
    provider_session_id: &str,
    in_folder: Option<InFolderGrant>,
    requested_mode: Option<&str>,
    catalog: &crate::ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
) -> crate::PermissionModeReport {
    let restored = crate::provider_setting_application::restored_permission_mode(
        requested_mode,
        catalog,
        behavior,
    );
    if let Some(grant) = in_folder
        && let Ok(mut grants) = grants.write()
    {
        grants.insert(
            provider_session_id.to_owned(),
            crate::in_folder_allowance::SessionInFolderGrant {
                grant,
                router_auto: restored.router_auto_active,
            },
        );
    }
    restored.report
}
