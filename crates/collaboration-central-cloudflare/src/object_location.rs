//! Physical namespace and Durable Object name for a core address.
use collaboration_central_core::ObjectAddress;
pub(crate) fn location(address: &ObjectAddress) -> (&'static str, String) {
    match address {
        ObjectAddress::Copy(project) => ("PROJECT_COPIES", project.to_string()),
        ObjectAddress::Fleet => ("FLEET", "fleet".into()),
        ObjectAddress::WorkspaceStore(workspace) => ("WORKSPACE_STORE", workspace.as_str().into()),
    }
}
