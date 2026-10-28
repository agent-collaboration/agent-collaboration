//! Read-only runtime inventory joined with the existing stored picker catalog.
use super::{SessionPickerIdentity, SessionPickerRecord, SessionRecord};
use crate::picker_runtime_status::{
    PickerRecordsSnapshot, PickerRuntimeCoverage, PickerRuntimeStatus,
};
use collaboration_client::protocol::{
    ChannelDescription, CodexGeneration, EndpointRef, NativeSessionListParams,
    NativeSessionObservation, NativeSessionScope, NativeSessionSource, NativeSessionView,
    ProviderSessionListParams,
};
use collaboration_client::{ClientError, CollaborationClient};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const MAX_RUNTIME_ROWS: usize = 4096;

#[path = "native_inventory_pager.rs"]
pub(super) mod native_inventory_pager;
use native_inventory_pager::NativeInventoryPager;

#[path = "native_inventory_binding.rs"]
mod native_inventory_binding;
use native_inventory_binding::{
    NativeBindingRejection, NativeEndpointSelector, NativeInventoryContext, bind_native_inventory,
};

pub(super) async fn load_runtime_records(
    client: &mut CollaborationClient,
    metadata: &[SessionPickerRecord],
    include_empty_sessions: bool,
) -> Result<(EndpointRef, Vec<SessionPickerRecord>), ClientError> {
    let (endpoint, generation) = selected_generation(client).await?;
    let known: BTreeMap<_, _> = metadata
        .iter()
        .map(|row| (row.session_id.as_str(), row))
        .collect();
    let mut rows = Vec::new();
    let mut pager = NativeInventoryPager::new(
        NativeSessionListParams {
            endpoint: endpoint.clone(),
            view: NativeSessionView::Loaded,
            scope: collaboration_client::protocol::NativeSessionScope::Any,
            source: collaboration_client::protocol::NativeSessionSource::All,
            include_empty_sessions,
            query: None,
            page_size: 100,
            cursor: None,
        },
        Some(generation.clone()),
    )?;
    while let Some(page) = pager.next_page(client).await? {
        for summary in page.sessions {
            let observed = SessionPickerRecord::from_native_summary(&summary);
            let id = String::from(summary.target.session_id.clone());
            let NativeSessionObservation::Runtime { .. } = summary.observation else {
                return Err(ClientError::Protocol("expected runtime observation"));
            };
            let mut row = if let Some(row) = known.get(id.as_str()) {
                (*row).clone()
            } else {
                let inspected = client.inspect_session(&summary.target).await?;
                if inspected.generation != generation {
                    return Err(ClientError::Protocol("runtime metadata generation changed"));
                }
                runtime_record(
                    &id,
                    &summary.title,
                    &String::from(summary.working_directory.clone()),
                    &inspected.thread,
                )
            };
            row.identity = observed.identity;
            if row.model.is_none() {
                row.model = observed.model;
            }
            if row.reasoning_effort.is_none() {
                row.reasoning_effort = observed.reasoning_effort;
            }
            row.provenance = super::SessionRowProvenance::ObservedHosted;
            row.runtime_status = observed.runtime_status;
            rows.push(row);
        }
    }
    let (current_endpoint, current_generation) = selected_generation(client).await?;
    if current_endpoint != endpoint || current_generation != generation {
        return Err(ClientError::Protocol(
            "runtime inventory generation changed",
        ));
    }
    Ok((endpoint, rows))
}

#[path = "provider_picker_inventory.rs"]
mod provider_picker_inventory;

#[cfg(test)]
async fn load_provider_records(
    client: &mut CollaborationClient,
) -> Result<provider_picker_inventory::ProviderPickerRecords, ClientError> {
    provider_picker_inventory::read_provider_records(
        client,
        tokio::time::Instant::now() + std::time::Duration::from_secs(5),
    )
    .await
}

async fn selected_generation(
    client: &mut CollaborationClient,
) -> Result<(EndpointRef, CodexGeneration), ClientError> {
    let inventory = client.list_endpoints().await?;
    let endpoint = EndpointRef {
        service_id: client.identity().service_id.clone(),
        endpoint_id: "codex-local"
            .to_owned()
            .try_into()
            .map_err(|_| ClientError::InvalidRequest("invalid default endpoint"))?,
    };
    let binding = bind_native_inventory(
        &inventory,
        client.identity(),
        &client.identity().service_id,
        NativeEndpointSelector::Exact(endpoint),
        NativeSessionView::Loaded,
    )
    .map_err(|reason| match reason {
        NativeBindingRejection::EndpointUnavailable
        | NativeBindingRejection::RuntimeUnavailable => {
            ClientError::Transport(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "native runtime unavailable",
            ))
        }
        _ => ClientError::Protocol("invalid native inventory binding"),
    })?;
    let NativeInventoryContext::Runtime(generation) = binding.context else {
        return Err(ClientError::Protocol("runtime generation unavailable"));
    };
    Ok((binding.endpoint, generation))
}

fn runtime_record(
    id: &str,
    fallback_title: &str,
    cwd: &str,
    thread: &serde_json::Value,
) -> SessionPickerRecord {
    let text = |key: &str| {
        thread
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let time = |key: &str| {
        thread
            .get(key)
            .and_then(serde_json::Value::as_i64)
            .and_then(|value| value.checked_mul(1000))
    };
    let name = text("name");
    let title = name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            if fallback_title.is_empty() {
                id.to_owned()
            } else {
                fallback_title.to_owned()
            }
        });
    let mut row = SessionPickerRecord::from_record(&SessionRecord {
        session_id: id.to_owned(),
        rollout_path: None,
        cwd: Some(cwd.to_owned()),
        provider: text("modelProvider"),
        model: text("model"),
        reasoning_effort: text("reasoningEffort"),
        source: thread.get("source").and_then(|source| {
            source
                .as_str()
                .map(str::to_owned)
                .or_else(|| serde_json::to_string(source).ok())
        }),
        thread_source: if thread
            .get("parentThreadId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| !id.is_empty())
        {
            Some("subagent".to_owned())
        } else {
            text("threadSource")
        },
        git_branch: thread
            .pointer("/gitInfo/branch")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        git_origin_url: thread
            .pointer("/gitInfo/originUrl")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        name,
        title: Some(title.clone()),
        preview: None,
        first_user_message: None,
        created_at_ms: time("createdAt"),
        updated_at_ms: time("updatedAt"),
        recency_at_ms: time("updatedAt"),
    });
    // Runtime inventory already chose its title; stored-session formatting is separate.
    row.title = title;
    row
}

/// Cache only remembered addresses/metadata; every refresh must re-establish live status.
#[derive(Default)]
pub(super) struct PickerRuntimeInventory {
    remembered: Vec<SessionPickerRecord>,
}
impl PickerRuntimeInventory {
    pub(super) async fn refresh(
        &mut self,
        directory: Option<&Path>,
        stored: Vec<SessionPickerRecord>,
        include_empty_sessions: bool,
    ) -> Result<PickerRecordsSnapshot, crate::presentation::session_picker::SourceInventoryRejection>
    {
        if directory.is_none() {
            self.remembered.retain(|row| !row.identity.is_provider());
        }
        let (provider_result, native_result) = if let Some(directory) = directory {
            let metadata = stored.clone();
            let providers = async {
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
                let mut client = tokio::time::timeout_at(
                    deadline,
                    CollaborationClient::connect(
                        directory,
                        "sessions-picker-providers",
                        env!("CARGO_PKG_VERSION"),
                    ),
                )
                .await
                .map_err(|_| ClientError::Timeout)??;
                provider_picker_inventory::read_provider_records(&mut client, deadline).await
            };
            let native = async {
                let mut client = CollaborationClient::connect(
                    directory,
                    "sessions-picker-native",
                    env!("CARGO_PKG_VERSION"),
                )
                .await?;

                load_runtime_records(&mut client, &metadata, include_empty_sessions).await
            };
            let deadline = std::time::Duration::from_secs(5);
            let (providers, native) =
                tokio::join!(providers, tokio::time::timeout(deadline, native));
            let native = native.unwrap_or(Err(ClientError::Timeout));
            for result in [providers.as_ref().map(|_| ()), native.as_ref().map(|_| ())] {
                if let Err(error) = result
                    && matches!(
                        super::source_inventory_failure::InventoryReadFailure::from_client(error),
                        super::source_inventory_failure::InventoryReadFailure::Invalid { .. }
                    )
                {
                    return Err(crate::presentation::session_picker::SourceInventoryRejection::InvalidInventory);
                }
            }
            (providers.ok(), native.ok())
        } else {
            (None, None)
        };
        let native_endpoint = native_result
            .as_ref()
            .map(|(endpoint, _)| endpoint)
            .or_else(|| {
                provider_result
                    .as_ref()
                    .and_then(|(endpoint, _, _)| endpoint.as_ref())
            });
        let native_live_coverage = provider_result
            .as_ref()
            .map(|(_, _, coverage)| coverage.clone())
            .unwrap_or_default();
        let provider_inventory_available = provider_result.is_some();
        let mut combined: BTreeMap<SessionPickerIdentity, SessionPickerRecord> = self
            .remembered
            .iter()
            .cloned()
            .chain(stored)
            .map(|mut row| {
                if let Some(endpoint) = native_endpoint
                    && matches!(row.identity, SessionPickerIdentity::LocalCodex(_))
                {
                    row = row.with_hosted_codex(endpoint);
                }
                row.runtime_status = PickerRuntimeStatus::Unknown;
                if row.identity.is_provider() && !provider_inventory_available {
                    row.provider_state = None;
                }
                row.recency = super::format_recency_at_ms(row.recency_at_ms);
                row.created = super::format_recency_at_ms(row.created_at_ms);
                (row.identity.clone(), row)
            })
            .collect();
        let runtime_coverage = if directory.is_none() {
            PickerRuntimeCoverage::LocalOnly
        } else if native_result.is_some() {
            PickerRuntimeCoverage::Available
        } else {
            PickerRuntimeCoverage::Unavailable
        };
        if native_result.is_some() || provider_result.is_some() {
            self.remembered.clear();
            if let Some((_, runtime)) = native_result {
                for row in runtime {
                    combined.insert(row.identity.clone(), row.clone());
                    self.remembered.push(row);
                }
            }
            if let Some((_, providers, _)) = provider_result {
                combined.retain(|identity, _| !identity.is_provider());
                for row in providers {
                    combined.insert(row.identity.clone(), row.clone());
                    self.remembered.push(row);
                }
            }
        } else {
            for row in &mut self.remembered {
                row.runtime_status = PickerRuntimeStatus::Unknown;
                if row.identity.is_provider() {
                    row.provider_state = None;
                }
            }
        }
        Ok(PickerRecordsSnapshot {
            native_live_coverage,
            records: combined.into_values().collect(),
            runtime_coverage,
        })
    }
}

#[cfg(test)]
#[path = "picker_runtime_inventory_tests.rs"]
mod tests;
