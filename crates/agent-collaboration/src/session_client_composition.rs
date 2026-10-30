use crate::CliContext;
use agent_session_inventory::sessions::source_ssh_protocol::{
    SourceNativeLaunchAvailability, SourceOperation, SourceReadError, SourceRejection, SourceReply,
};
use agent_session_inventory::source_inventory_request::PickerSourceContext;
use agent_session_tui::session_picker::{NewClientInventoryLoader, SessionClientPorts};
use collaboration_client::protocol::{EndpointInventory, UuidIdentity};
use std::sync::Arc;

pub(crate) fn production_ports(context: CliContext) -> SessionClientPorts {
    SessionClientPorts {
        inventory_loader: production_loader(context.clone()),
        details_inspector: crate::details_client_composition::production_inspector(context.clone()),
        executor: production_executor(context),
    }
}

pub(crate) fn production_loader(context: CliContext) -> NewClientInventoryLoader {
    let service_directory =
        crate::session_environment::collaboration_service_directory(&context).ok();
    let codex_home = crate::session_environment::codex_home(&context).ok();
    let home_directory = context.env_var("HOME").map(std::path::PathBuf::from);
    let executable_path = context.env_var("PATH").map(str::to_owned);

    Arc::new(move |source| {
        let service_directory = service_directory.clone();
        let codex_home = codex_home.clone();
        let home_directory = home_directory.clone();
        let executable_path = executable_path.clone();
        Box::pin(async move {
            let (expected, inventory, native_claude) = read_client_inventory(
                service_directory.as_deref(),
                codex_home.as_deref(),
                home_directory.as_deref(),
                executable_path.as_deref(),
                source,
            )
            .await?;
            agent_session_tui::session_picker::new_session_client_inventory::qualify_capabilities(
                &expected,
                inventory,
                native_claude,
            )
        })
    })
}

pub(super) async fn read_client_inventory(
    service_directory: Option<&std::path::Path>,
    codex_home: Option<&std::path::Path>,
    home_directory: Option<&std::path::Path>,
    executable_path: Option<&str>,
    source: PickerSourceContext,
) -> Result<
    (
        UuidIdentity,
        EndpointInventory,
        SourceNativeLaunchAvailability,
    ),
    SourceReadError,
> {
    let (expected, reply) = match source {
        PickerSourceContext::ConfiguredHosted(profile) => {
            let expected = profile.service_id.clone();
            let reply = agent_session_inventory::sessions::source_ssh_transport::read_source(
                &profile,
                SourceOperation::ClientInventory,
            )
            .await?;
            (expected, reply)
        }
        PickerSourceContext::DefaultHosted => {
            agent_session_inventory::sessions::source_ssh_executor::inspect_local_clients(
                service_directory.ok_or(SourceReadError::Rejected(
                    SourceRejection::SourceUnavailable,
                ))?,
                codex_home,
                home_directory,
                executable_path,
            )
            .await?
        }
        PickerSourceContext::LocalCodex => {
            return Err(SourceReadError::Rejected(
                SourceRejection::UnsupportedConnection,
            ));
        }
    };
    let SourceReply::ClientInventory {
        inventory,
        native_claude,
    } = reply
    else {
        return Err(SourceReadError::InvalidReply);
    };
    Ok((expected, inventory, native_claude))
}

use agent_session_inventory::sessions::source_client_action::SessionClientExecutor;
pub(crate) fn production_executor(context: CliContext) -> SessionClientExecutor {
    let service_directory =
        crate::session_environment::collaboration_service_directory(&context).ok();
    let codex_home = crate::session_environment::codex_home(&context).ok();
    let home_directory = context.env_var("HOME").map(std::path::PathBuf::from);
    let executable_path = context.env_var("PATH").map(str::to_owned);
    Arc::new(move |request| {
        let service_directory = service_directory.clone();
        let codex_home = codex_home.clone();
        let home_directory = home_directory.clone();
        let executable_path = executable_path.clone();
        Box::pin(async move {
            agent_session_inventory::sessions::source_client_action::execute_client_action(
                service_directory.as_deref(),
                codex_home.as_deref(),
                home_directory.as_deref(),
                executable_path.as_deref(),
                request,
            )
            .await
        })
    })
}
