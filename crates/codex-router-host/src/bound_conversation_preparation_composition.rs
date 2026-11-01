//! Host facts prepare the bound helper; the registrar alone refreshes and supplies its actual starter.
use collaboration_client::{ConversationCreateActor, ConversationCreateInput};
use collaboration_contracts::{
    HarnessDeclaredLaunchInput, HarnessDeclaredSettings, HarnessForkInput, HarnessLaunchFailure,
    HarnessMcpConfigurationIdentity,
};
use collaboration_mcp::{
    BoundConversationPreparation, BoundConversationPreparationFuture, BoundConversationPrepared,
};
use collaboration_protocol::ProviderIdentity;
use collaboration_service::fleet_session_registrar::{
    BoundFleetSession, FleetSessionRegistrar, PrepareSessionStart,
};
use fleet_identity_protocol::MachineEnvironment;
use std::sync::Arc;

pub struct HostBoundConversationPreparation {
    registrar: Arc<FleetSessionRegistrar>,
    confirmed_machine_environment: MachineEnvironment,
    ids: Arc<dyn runtime_primitives::IdSource + Send + Sync>,
}
impl HostBoundConversationPreparation {
    #[must_use]
    pub fn new(
        registrar: Arc<FleetSessionRegistrar>,
        confirmed_machine_environment: MachineEnvironment,
        ids: Arc<dyn runtime_primitives::IdSource + Send + Sync>,
    ) -> Self {
        Self {
            registrar,
            confirmed_machine_environment,
            ids,
        }
    }
}
impl BoundConversationPreparation for HostBoundConversationPreparation {
    fn prepare<'a>(
        &'a self,
        caller: &'a BoundFleetSession,
        input: &'a ConversationCreateInput,
    ) -> BoundConversationPreparationFuture<'a> {
        Box::pin(async move {
            let declared = declared_conversation_launch_input(input)?;
            let identity = declared
                .identity()
                .map_err(|_| HarnessLaunchFailure::InvalidInput)?;
            let facts = crate::harness_launch_facts::declared_host_launch(
                &input.operation_id,
                &declared,
                &self.confirmed_machine_environment,
                self.ids.as_ref(),
            )
            .await?;
            match self
                .registrar
                .prepare_bound_helper(caller, &facts, &identity)
                .await
                .map_err(crate::harness_launch_composition::launch_failure)?
            {
                PrepareSessionStart::Prepared(_) | PrepareSessionStart::Existing(_) => {
                    Ok(BoundConversationPrepared)
                }
                PrepareSessionStart::AccessEnrollment => Err(HarnessLaunchFailure::Unavailable),
            }
        })
    }
}
fn permission_identity(
    actor: &ConversationCreateActor,
) -> Result<ProviderIdentity, HarnessLaunchFailure> {
    match actor {
        ConversationCreateActor::Session(session) => Ok(session.clone().into()),
        ConversationCreateActor::Typed(actor) => ProviderIdentity::from_board_identity(actor)
            .map_err(|_| HarnessLaunchFailure::InvalidInput),
    }
}
pub(crate) fn declared_conversation_launch_input(
    input: &ConversationCreateInput,
) -> Result<HarnessDeclaredLaunchInput, HarnessLaunchFailure> {
    let created_by = permission_identity(&input.created_by)?;
    let approver = input
        .approver
        .as_ref()
        .map(permission_identity)
        .transpose()?
        .unwrap_or_else(|| created_by.clone());
    let settings = if String::from(input.endpoint.endpoint_id.clone()) == "codex-local" {
        if input.mode.is_some()
            || input.generation.is_some()
            || !matches!(&created_by, ProviderIdentity::Session(_))
            || !matches!(&approver, ProviderIdentity::Session(_))
        {
            return Err(HarnessLaunchFailure::InvalidInput);
        }
        HarnessDeclaredSettings::Native {
            model: input
                .model
                .clone()
                .map(TryInto::try_into)
                .transpose()
                .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
            effort: input
                .effort
                .clone()
                .map(TryInto::try_into)
                .transpose()
                .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
            permissions: collaboration_contracts::HarnessNativePermissions::Explicit {
                access: input.access,
                created_by,
                approver,
            },
            additional_directories: Vec::new(),
            fork: input.fork.as_ref().map(|session_id| HarnessForkInput {
                source: collaboration_protocol::SessionRef {
                    endpoint: input.endpoint.clone(),
                    session_id: session_id.clone(),
                },
                through_turn_id: None,
            }),
            root_message_id: input.root_message_id.clone(),
        }
    } else {
        if input.fork.is_some() || input.root_message_id.is_some() {
            return Err(HarnessLaunchFailure::InvalidInput);
        }
        HarnessDeclaredSettings::Provider {
            model: input
                .model
                .clone()
                .map(TryInto::try_into)
                .transpose()
                .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
            effort: input
                .effort
                .clone()
                .map(TryInto::try_into)
                .transpose()
                .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
            mode: input
                .mode
                .clone()
                .map(TryInto::try_into)
                .transpose()
                .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
            access: input.access,
            created_by,
            approver,
        }
    };
    Ok(HarnessDeclaredLaunchInput {
        project_id: input.project_id.clone(),
        endpoint: input.endpoint.clone(),
        working_directory: input
            .working_directory
            .to_str()
            .ok_or(HarnessLaunchFailure::InvalidInput)?
            .to_owned()
            .try_into()
            .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
        settings,
        user_mcp: HarnessMcpConfigurationIdentity::empty(),
    })
}
