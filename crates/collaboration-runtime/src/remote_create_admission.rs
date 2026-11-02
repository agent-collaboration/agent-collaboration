//! Target-local preflight for work received through the authenticated central session.
use crate::{EndpointDirectory, NativeControlBackend};
use collaboration_protocol::{
    ChannelDescription, CodexGeneration, ConversationOperationFailure,
    ConversationOperationFailureKind, ConversationOperationFailureStage, EndpointAvailability,
    EndpointDescription, EndpointRef, NonEmptyText, OperationId, ProviderCapabilityName,
    ProviderCapabilityStatus, ProviderIdentity, ProviderOperationEffect, RemoteCreateRefusal,
    SessionCreateEntry, SessionRef, UuidIdentity,
};

#[path = "remote_create_native_models.rs"]
mod native_models;
use native_models::{NativeCreateSettingsError, validate_native_settings};

pub struct RemoteCreateAdmissionInputs {
    pub service_id: UuidIdentity,
    pub owner_human_id: Option<message_board::HumanId>,
    pub remote_create_default_approver: Option<SessionRef>,
    pub endpoints: EndpointDirectory,
    pub native_backend: Option<NativeControlBackend>,
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteCreateConfigurationError {
    #[error("the interim remote-create default approver must be a Session on this service")]
    DefaultApproverNotLocal,
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteCreateAdmissionError {
    #[error("remote create refused by target policy")]
    Refused(RemoteCreateRefusal),
    #[error("remote-create diagnostic is invalid")]
    InvalidDiagnostic,
}

impl From<RemoteCreateRefusal> for RemoteCreateAdmissionError {
    fn from(refusal: RemoteCreateRefusal) -> Self {
        Self::Refused(refusal)
    }
}

pub struct RemoteCreateAdmission {
    inputs: RemoteCreateAdmissionInputs,
}

#[derive(Clone, Debug)]
pub enum PreparedRemoteCreateActors {
    Codex {
        creator: SessionRef,
        approver: SessionRef,
        generation: CodexGeneration,
    },
    Provider {
        creator: ProviderIdentity,
        approver: ProviderIdentity,
    },
}

#[derive(Clone, Debug)]
pub struct PreparedRemoteCreate {
    entry: SessionCreateEntry,
    actors: PreparedRemoteCreateActors,
    local_operation_id: OperationId,
}

impl PreparedRemoteCreate {
    pub fn entry(&self) -> &SessionCreateEntry {
        &self.entry
    }
    pub fn actors(&self) -> &PreparedRemoteCreateActors {
        &self.actors
    }
    pub fn local_operation_id(&self) -> &OperationId {
        &self.local_operation_id
    }
}

impl RemoteCreateAdmission {
    pub fn new(
        inputs: RemoteCreateAdmissionInputs,
    ) -> Result<Self, RemoteCreateConfigurationError> {
        if inputs
            .remote_create_default_approver
            .as_ref()
            .is_some_and(|session| session.endpoint.service_id != inputs.service_id)
        {
            return Err(RemoteCreateConfigurationError::DefaultApproverNotLocal);
        }
        Ok(Self { inputs })
    }

    pub async fn prepare_authenticated_work(
        &self,
        entry: &SessionCreateEntry,
    ) -> Result<PreparedRemoteCreate, RemoteCreateAdmissionError> {
        let local_operation_id = OperationId::try_from(entry.work_item_id.as_str().to_owned())
            .map_err(|_| {
                local_refusal(
                    &entry.operation_id,
                    &entry.request.endpoint,
                    ConversationOperationFailureKind::ProtocolViolation,
                    "invalid work operation identity",
                )
            })?;
        let refusal = |kind, message| {
            local_refusal(&local_operation_id, &entry.request.endpoint, kind, message)
        };
        if entry.request.created_by.session().is_some_and(|creator| {
            String::from(creator.endpoint.service_id.clone()) != entry.source_installation.as_str()
        }) {
            return Err(RemoteCreateRefusal::OriginMismatch.into());
        }
        if entry.request.endpoint.service_id != self.inputs.service_id {
            return Err(refusal(
                ConversationOperationFailureKind::InvalidRequest,
                "the work target does not belong to this service",
            ));
        }
        let endpoint = self
            .inputs
            .endpoints
            .read_endpoint(&entry.request.endpoint)
            .map_err(|_| {
                refusal(
                    ConversationOperationFailureKind::Unavailable,
                    "endpoint directory unavailable",
                )
            })?
            .ok_or_else(|| {
                refusal(
                    ConversationOperationFailureKind::Unavailable,
                    "target endpoint is unavailable",
                )
            })?;
        if !matches!(
            endpoint.availability,
            EndpointAvailability::Available { .. }
        ) {
            return Err(refusal(
                ConversationOperationFailureKind::Unavailable,
                "target endpoint is unavailable",
            ));
        }
        let kind = endpoint_kind(&endpoint).ok_or_else(|| {
            refusal(
                ConversationOperationFailureKind::UnsupportedCapability,
                "target endpoint does not advertise creation",
            )
        })?;
        if kind == EndpointKind::Codex
            && matches!(entry.request.created_by, ProviderIdentity::Human { .. })
        {
            return Err(RemoteCreateRefusal::CreatorNotSupported.into());
        }
        let approver = entry
            .request
            .approver
            .clone()
            .or_else(|| {
                self.inputs
                    .remote_create_default_approver
                    .clone()
                    .map(ProviderIdentity::Session)
            })
            .ok_or(RemoteCreateRefusal::ApproverRequired)?;
        let eligible_approver = match &approver {
            ProviderIdentity::Session(session) => {
                session.endpoint.service_id == self.inputs.service_id
            }
            ProviderIdentity::Human { human_id } => {
                kind == EndpointKind::Provider
                    && self.inputs.owner_human_id.as_ref() == Some(human_id)
            }
        };
        if !eligible_approver {
            return Err(RemoteCreateRefusal::ApproverNotSupported.into());
        }
        let directory = String::from(entry.request.working_directory.clone());
        let metadata = tokio::fs::metadata(directory).await.map_err(|_| {
            refusal(
                ConversationOperationFailureKind::InvalidRequest,
                "working directory is unavailable",
            )
        })?;
        if !metadata.is_dir() {
            return Err(refusal(
                ConversationOperationFailureKind::InvalidRequest,
                "working directory is not a directory",
            ));
        }
        let actors = match kind {
            EndpointKind::Provider => PreparedRemoteCreateActors::Provider {
                creator: entry.request.created_by.clone(),
                approver,
            },
            EndpointKind::Codex => {
                let ProviderIdentity::Session(creator) = &entry.request.created_by else {
                    return Err(RemoteCreateRefusal::CreatorNotSupported.into());
                };
                let ProviderIdentity::Session(approver) = approver else {
                    return Err(RemoteCreateRefusal::ApproverNotSupported.into());
                };
                let settings = entry.request.settings.as_ref().ok_or_else(|| {
                    refusal(
                        ConversationOperationFailureKind::InvalidRequest,
                        "Codex creation requires model and effort",
                    )
                })?;
                let backend = self
                    .inputs
                    .native_backend
                    .as_ref()
                    .filter(|backend| backend.endpoint == entry.request.endpoint)
                    .ok_or_else(|| {
                        refusal(
                            ConversationOperationFailureKind::Unavailable,
                            "native endpoint is unavailable",
                        )
                    })?;
                let generation = validate_native_settings(backend, settings).await.map_err(
                    |error| match error {
                        NativeCreateSettingsError::InvalidInput => refusal(
                            ConversationOperationFailureKind::InvalidRequest,
                            "Codex creation requires model and effort without a provider mode",
                        ),
                        NativeCreateSettingsError::ModelNotAdvertised => refusal(
                            ConversationOperationFailureKind::InvalidSetting,
                            "requested model is not advertised by the native endpoint",
                        ),
                        NativeCreateSettingsError::EffortNotAdvertised => refusal(
                            ConversationOperationFailureKind::InvalidSetting,
                            "requested effort is not advertised for the native model",
                        ),
                        NativeCreateSettingsError::Unavailable => refusal(
                            ConversationOperationFailureKind::Unavailable,
                            "native model advertisement is unavailable",
                        ),
                        NativeCreateSettingsError::InvalidAdvertisement => refusal(
                            ConversationOperationFailureKind::ProtocolViolation,
                            "native model advertisement is invalid",
                        ),
                    },
                )?;
                PreparedRemoteCreateActors::Codex {
                    creator: creator.clone(),
                    approver,
                    generation,
                }
            }
        };
        Ok(PreparedRemoteCreate {
            entry: entry.clone(),
            actors,
            local_operation_id,
        })
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum EndpointKind {
    Codex,
    Provider,
}

fn endpoint_kind(endpoint: &EndpointDescription) -> Option<EndpointKind> {
    let codex = endpoint
        .channels
        .iter()
        .any(|channel| matches!(channel, ChannelDescription::Acp { .. }));
    let provider = endpoint.channels.iter().any(|channel| matches!(channel,
        ChannelDescription::ExternalProvider { capabilities, .. } if capabilities.as_slice().iter().any(|capability|
            capability.name == ProviderCapabilityName::Create && capability.status == ProviderCapabilityStatus::Supported)));
    match (codex, provider) {
        (true, false) => Some(EndpointKind::Codex),
        (false, true) => Some(EndpointKind::Provider),
        _ => None,
    }
}

fn local_refusal(
    operation_id: &OperationId,
    endpoint: &EndpointRef,
    kind: ConversationOperationFailureKind,
    message: &'static str,
) -> RemoteCreateAdmissionError {
    let Ok(message) = NonEmptyText::try_from(message.to_owned()) else {
        return RemoteCreateAdmissionError::InvalidDiagnostic;
    };
    RemoteCreateAdmissionError::Refused(RemoteCreateRefusal::Local {
        failure: Box::new(ConversationOperationFailure {
            kind,
            stage: ConversationOperationFailureStage::Validation,
            effect: ProviderOperationEffect::None,
            message,
            operation_id: Some(operation_id.clone()),
            invalid_setting: None,
            provider_code: None,
            target: None,
            endpoint: Some(endpoint.clone()),
            availability: None,
        }),
    })
}

#[cfg(test)]
#[path = "remote_create_admission_tests.rs"]
mod tests;
