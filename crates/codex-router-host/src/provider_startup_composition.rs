//! Isolated provider admission and endpoint publication for one Host start.
use crate::{
    ExternalProviderBinding, ExternalProviderLaunchBinding, ExternalProviderRuntime,
    ExternalProviderSupervisor,
};
use collaboration_protocol::{
    ChannelDescription, CodexGeneration, EndpointAvailability, EndpointDescription, EndpointId,
    EndpointRef, GenerationNumber, NonEmptyText, ObservationTimestamp, ProviderBindingId,
    ProviderBindingIdentity, ProviderCapabilities, ProviderCapability, ProviderCapabilityEvidence,
    ProviderCapabilityName, ProviderCapabilityStatus, ProviderKind, ProviderRuntimeIdentity,
    ProviderTransport, UuidIdentity,
};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionEventHub;
use collaboration_runtime::new_service_uuid;
use std::{io, sync::Arc};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalProviderStartup {
    Launch(ExternalProviderLaunchBinding),
    Unavailable {
        provider: ProviderKind,
        reason: NonEmptyText,
        fix: NonEmptyText,
    },
}

impl ExternalProviderStartup {
    /// A launch that starts its auto sessions with these work roots (D1 R18);
    /// an unavailable provider is unchanged.
    #[must_use]
    pub fn with_work_roots(self, work_roots: &[std::path::PathBuf]) -> Self {
        match self {
            Self::Launch(binding) => Self::Launch(binding.with_work_roots(work_roots.to_vec())),
            unavailable @ Self::Unavailable { .. } => unavailable,
        }
    }

    pub fn unavailable(
        provider: ProviderKind,
        reason: String,
        fix: String,
    ) -> Result<Self, &'static str> {
        Ok(Self::Unavailable {
            provider,
            reason: NonEmptyText::try_from(reason)?,
            fix: NonEmptyText::try_from(fix)?,
        })
    }
}

pub(crate) struct ProviderStartupComposition {
    pub supervisor: Option<Arc<ExternalProviderSupervisor>>,
    pub hub: Option<Arc<ProviderSessionEventHub>>,
    pub endpoints: Vec<EndpointDescription>,
    pub retirements: Vec<(CancellationToken, EndpointDescription)>,
    /// The started Claude endpoint and the setting its startup carried.
    pub cache_warm: Option<(EndpointRef, crate::CacheWarmSetting)>,
}

pub(crate) async fn compose_provider_startup(
    configured: Vec<ExternalProviderStartup>,
    store: Option<Arc<Mutex<ProviderOperationStore>>>,
    service_id: &UuidIdentity,
    service_epoch: &UuidIdentity,
    mcp_url: &str,
    scratch_root: Option<std::path::PathBuf>,
    hook_directory: Option<std::path::PathBuf>,
) -> io::Result<ProviderStartupComposition> {
    let hub = store
        .as_ref()
        .map(|store| Arc::new(ProviderSessionEventHub::new(Arc::clone(store))));
    let mut bindings = Vec::new();
    let mut endpoints = Vec::with_capacity(configured.len());
    let mut retirements = Vec::new();
    let mut cache_warm = None;
    let mut work_roots = Vec::new();
    let mut installed_hook = None;
    for provider in configured {
        match provider {
            ExternalProviderStartup::Unavailable {
                provider,
                reason,
                fix,
            } => {
                endpoints.push(unavailable_endpoint(service_id, provider, reason, fix)?);
            }
            ExternalProviderStartup::Launch(binding) if store.is_none() => {
                endpoints.push(unavailable_endpoint(
                    service_id,
                    binding.provider,
                    text("provider operation storage unavailable")?,
                    text("repair the router root storage and restart the Host")?,
                )?);
            }
            ExternalProviderStartup::Launch(binding) => {
                let executable = binding.launch.executable.display().to_string();
                let mut launch = binding.launch;
                if binding.provider == ProviderKind::ClaudeCode
                    && !crate::global_claude_executable::bind_global_claude_executable(
                        &mut launch.environment,
                    )
                {
                    endpoints.push(unavailable_endpoint(
                        service_id,
                        binding.provider,
                        text("global Claude executable is unavailable on the Host launch PATH")?,
                        text("install Claude Code globally, ensure claude is on the Host PATH, then restart the Host")?,
                    )?);
                    continue;
                }
                launch.persistence_target = match binding.provider {
                    ProviderKind::Cursor => {
                        acp_client_runtime::ProviderPersistenceTarget::CursorAllowlist
                    }
                    ProviderKind::ClaudeCode => {
                        acp_client_runtime::ProviderPersistenceTarget::ClaudeSettingsRule
                    }
                };
                let endpoint = EndpointRef {
                    service_id: service_id.clone(),
                    endpoint_id: binding.endpoint_id.clone(),
                };
                let mut claude_node = None;
                let behavior: Arc<dyn acp_client_runtime::ProviderBehavior> = match binding.provider
                {
                    ProviderKind::Cursor => Arc::new(acp_provider_adapters::CursorAcpBehavior),
                    ProviderKind::ClaudeCode => {
                        let prepared = acp_provider_adapters::prepare_claude_launch(launch).await;
                        match &prepared.qualification {
                            Ok(identity) => tracing::info!(
                                node = %identity.node.display(), node_version = %identity.node_version,
                                library = %identity.library.display(), sdk = %identity.sdk.display(),
                                "qualified Claude ACP metadata launch"
                            ),
                            Err(reason) => tracing::debug!(
                                ?reason,
                                "Claude skill metadata launch is unqualified"
                            ),
                        }
                        claude_node = prepared
                            .qualification
                            .as_ref()
                            .ok()
                            .map(|identity| identity.node.clone());
                        launch = prepared.launch;
                        Arc::new(acp_provider_adapters::ClaudeAcpBehavior::new(
                            prepared.skill_source,
                        ))
                    }
                };
                // The Node a qualified Claude launch runs under also runs its
                // work-roots hook (D1 R18).
                let launch_node = claude_node
                    .or_else(|| crate::provider_work_roots::launch_node(&launch.executable));
                let hub_endpoint = message_board::SessionEndpointRef {
                    service_id: message_board::ServiceId::try_from(String::from(
                        service_id.clone(),
                    ))
                    .map_err(io::Error::other)?,
                    endpoint_id: message_board::EndpointId::try_from(String::from(
                        binding.endpoint_id.clone(),
                    ))
                    .map_err(io::Error::other)?,
                };
                let auto_realization =
                    crate::provider_operation_settlement::provider_auto_realization(
                        behavior.auto_permission_mode().realization,
                    );
                let runtime = ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
                    launch,
                    provider_model_picker(binding.provider),
                    "router-collaboration",
                    mcp_url.to_owned(),
                    Arc::clone(hub.as_ref().ok_or_else(|| {
                        io::Error::other("provider Session hub unavailable for configured launch")
                    })?),
                    hub_endpoint,
                    behavior,
                )
                .await;
                match runtime {
                    Ok(runtime) => {
                        runtime
                            .set_endpoint_id(String::from(binding.endpoint_id.clone()))
                            .await;
                        let generation_number = generation_number(binding.provider)?;
                        let runtime_name =
                            runtime.admission().runtime_name.clone().unwrap_or_else(|| {
                                match binding.provider {
                                    ProviderKind::ClaudeCode => "claude-code",
                                    ProviderKind::Cursor => "cursor",
                                }
                                .to_owned()
                            });
                        let runtime_identity = ProviderRuntimeIdentity {
                            provider: binding.provider,
                            runtime_name: text(&runtime_name)?,
                            runtime_version: runtime
                                .admission()
                                .runtime_version
                                .clone()
                                .map(NonEmptyText::try_from)
                                .transpose()
                                .map_err(io::Error::other)?,
                        };
                        let capabilities = provider_capabilities(
                            runtime.admission().supports_load,
                            runtime.admission().supports_mcp_http,
                        )?;
                        let binding_id =
                            ProviderBindingId::try_from(String::from(new_service_uuid()?))
                                .map_err(io::Error::other)?;
                        let generation = CodexGeneration {
                            service_epoch: service_epoch.clone(),
                            generation: generation_number,
                        };
                        let identity = ProviderBindingIdentity {
                            endpoint: endpoint.clone(),
                            binding_id: binding_id.clone(),
                            runtime: runtime_identity.clone(),
                            transport: ProviderTransport::StdioAcp,
                            generation,
                            capabilities: capabilities.clone(),
                        };
                        let description = EndpointDescription {
                            endpoint,
                            label: binding.label,
                            availability: EndpointAvailability::Available {
                                observed_at: observed_at()?,
                            },
                            channels: vec![ChannelDescription::ExternalProvider {
                                transport: ProviderTransport::StdioAcp,
                                binding_id,
                                binding_generation: generation_number,
                                runtime: runtime_identity,
                                capabilities,
                                auto_realization: Some(auto_realization),
                            }],
                        };
                        if let (ProviderKind::ClaudeCode, Some(setting)) =
                            (binding.provider, binding.cache_warm)
                        {
                            cache_warm = Some((description.endpoint.clone(), setting));
                        }
                        // Claude's sessions in auto need the work-roots hook,
                        // whatever the roots list holds: their working, scratch
                        // and project tmp/ folders still bound them (D1 R18).
                        let hook = match binding.provider {
                            ProviderKind::Cursor => {
                                crate::provider_work_roots::WorkRootsHook::NotNeeded
                            }
                            ProviderKind::ClaudeCode => claude_work_roots_hook(
                                launch_node,
                                hook_directory.as_deref(),
                                &mut installed_hook,
                            ),
                        };
                        work_roots.push((
                            description.endpoint.clone(),
                            crate::provider_work_roots::ProviderWorkRoots {
                                roots: binding.work_roots.clone(),
                                hook,
                            },
                        ));
                        retirements.push((runtime.retirement(), description.clone()));
                        endpoints.push(description);
                        bindings.push(ExternalProviderBinding { identity, runtime });
                    }
                    Err(error) => {
                        let fix = match binding.provider {
                            ProviderKind::ClaudeCode => {
                                "install claude-agent-acp or set the Claude executable in the settings.jsonc providers section or --claude-acp-executable"
                            }
                            ProviderKind::Cursor => {
                                "install Cursor agent or set the Cursor executable in the settings.jsonc providers section or --cursor-acp-executable"
                            }
                        };
                        endpoints.push(unavailable_endpoint(
                            service_id,
                            binding.provider,
                            text(&provider_startup_failure_reason(&executable, &error))?,
                            text(fix)?,
                        )?);
                    }
                }
            }
        }
    }
    let supervisor = match store {
        Some(store) if !endpoints.is_empty() => Some(Arc::new(
            ExternalProviderSupervisor::new_with_hub(bindings, store, hub.clone(), scratch_root)
                .map_err(io::Error::other)?,
        )),
        _ => None,
    };
    if let Some(supervisor) = &supervisor {
        for (endpoint, roots) in work_roots {
            supervisor.set_work_roots(&endpoint, roots);
        }
    }
    Ok(ProviderStartupComposition {
        supervisor,
        hub,
        endpoints,
        retirements,
        cache_warm,
    })
}

fn provider_startup_failure_reason(
    executable: &str,
    error: &crate::ExternalProviderRuntimeError,
) -> String {
    format!("provider launch or initialize failed for {executable}: {error}")
}

fn unavailable_endpoint(
    service_id: &UuidIdentity,
    provider: ProviderKind,
    reason: NonEmptyText,
    fix: NonEmptyText,
) -> io::Result<EndpointDescription> {
    let (endpoint_id, label) = match provider {
        ProviderKind::ClaudeCode => ("claude-local", "Claude Code"),
        ProviderKind::Cursor => ("cursor-local", "Cursor"),
    };
    Ok(EndpointDescription {
        endpoint: EndpointRef {
            service_id: service_id.clone(),
            endpoint_id: EndpointId::try_from(endpoint_id.to_owned()).map_err(io::Error::other)?,
        },
        label: text(label)?,
        availability: EndpointAvailability::Unavailable {
            observed_at: observed_at()?,
            reason,
            fix: Some(fix),
        },
        channels: Vec::new(),
    })
}

fn observed_at() -> io::Result<ObservationTimestamp> {
    ObservationTimestamp::try_from(
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    )
    .map_err(io::Error::other)
}

fn text(value: &str) -> io::Result<NonEmptyText> {
    NonEmptyText::try_from(value.to_owned()).map_err(io::Error::other)
}

fn generation_number(provider: ProviderKind) -> io::Result<GenerationNumber> {
    let ordinal = match provider {
        ProviderKind::ClaudeCode => 1,
        ProviderKind::Cursor => 2,
    };
    GenerationNumber::try_from(ordinal).map_err(io::Error::other)
}

fn provider_model_picker(provider: ProviderKind) -> acp_client_runtime::ProviderModelPicker {
    match provider {
        ProviderKind::Cursor => acp_client_runtime::ProviderModelPicker::CursorParameterized,
        ProviderKind::ClaudeCode => acp_client_runtime::ProviderModelPicker::Standard,
    }
}

#[cfg(test)]
#[path = "provider_startup_model_picker_test.rs"]
mod model_picker_tests;

fn provider_capabilities(
    supports_load: bool,
    supports_mcp_http: bool,
) -> io::Result<ProviderCapabilities> {
    ProviderCapabilities::try_from(vec![
        ProviderCapability {
            name: ProviderCapabilityName::Create,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::Advertised,
        },
        ProviderCapability {
            name: ProviderCapabilityName::Prompt,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::Advertised,
        },
        ProviderCapability {
            name: ProviderCapabilityName::Load,
            status: if supports_load {
                ProviderCapabilityStatus::Supported
            } else {
                ProviderCapabilityStatus::Unsupported
            },
            evidence: if supports_load {
                ProviderCapabilityEvidence::Advertised
            } else {
                ProviderCapabilityEvidence::NotAvailable
            },
        },
        ProviderCapability {
            name: ProviderCapabilityName::Cancel,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::Advertised,
        },
        ProviderCapability {
            name: ProviderCapabilityName::Permissions,
            status: ProviderCapabilityStatus::Unverified,
            evidence: ProviderCapabilityEvidence::NotAvailable,
        },
        ProviderCapability {
            name: ProviderCapabilityName::CollaborationMcp,
            status: if supports_mcp_http {
                ProviderCapabilityStatus::Supported
            } else {
                ProviderCapabilityStatus::Unsupported
            },
            evidence: if supports_mcp_http {
                ProviderCapabilityEvidence::Advertised
            } else {
                ProviderCapabilityEvidence::NotAvailable
            },
        },
        ProviderCapability {
            name: ProviderCapabilityName::CallerDetach,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::RouterQualified,
        },
    ])
    .map_err(io::Error::other)
}

#[cfg(test)]
#[path = "provider_startup_composition_test.rs"]
mod tests;

/// The work-roots hook for Claude's sessions: installed once per Host and run
/// by the launch's Node, or why it can't be.
fn claude_work_roots_hook(
    node: Option<std::path::PathBuf>,
    hook_directory: Option<&std::path::Path>,
    installed: &mut Option<std::path::PathBuf>,
) -> crate::provider_work_roots::WorkRootsHook {
    use crate::provider_work_roots::{AskOutsideHook, WorkRootsHook};
    let Some(node) = node else {
        tracing::warn!("the Claude launch runs no known Node, so Claude can't start in auto");
        return WorkRootsHook::Unavailable("the Claude launch runs no known Node".to_owned());
    };
    let Some(directory) = hook_directory else {
        return WorkRootsHook::Unavailable("the Host has no folder for the hook".to_owned());
    };
    let hook = match installed {
        Some(hook) => hook.clone(),
        None => match crate::provider_work_roots::install_work_roots_hook(directory) {
            Ok(hook) => {
                *installed = Some(hook.clone());
                hook
            }
            Err(error) => {
                tracing::warn!(%error, "the Claude work-roots hook could not be installed, so Claude can't start in auto");
                return WorkRootsHook::Unavailable(format!(
                    "the hook could not be installed: {error}"
                ));
            }
        },
    };
    WorkRootsHook::Ready(AskOutsideHook { node, hook })
}
