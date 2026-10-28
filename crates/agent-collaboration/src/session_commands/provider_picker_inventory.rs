//! Independent local Stored and native-live provider partitions, within one read budget.
use super::{
    BTreeSet, ChannelDescription, ClientError, CollaborationClient, EndpointRef, MAX_RUNTIME_ROWS,
    NativeBindingRejection, NativeEndpointSelector, NativeSessionScope, NativeSessionSource,
    NativeSessionView, ProviderSessionListParams, SessionPickerRecord, bind_native_inventory,
};
use crate::picker_runtime_status::{
    NativeCatalogueView, NativeLiveCoverage, NativeLiveReadState, NativeLiveUnavailableReason,
};
use crate::presentation::session_picker::SourceInventoryBinding;
use crate::sessions::source_inventory_failure::InventoryReadFailure;
use collaboration_client::protocol::EndpointDescription;
use tokio::time::{Instant, timeout_at};

pub(super) type ProviderPickerRecords = (
    Option<EndpointRef>,
    Vec<SessionPickerRecord>,
    Vec<NativeLiveCoverage>,
);

enum ProviderReadPartition {
    ManagedStored(EndpointDescription),
    NativeClaude(EndpointDescription, NativeCatalogueView),
}

pub(super) async fn read_provider_records(
    client: &mut CollaborationClient,
    deadline: Instant,
) -> Result<ProviderPickerRecords, ClientError> {
    let inventory = timeout_at(deadline, client.list_endpoints())
        .await
        .map_err(|_| ClientError::Timeout)??;
    let binding = SourceInventoryBinding::from_inventory(&client.identity().service_id, &inventory)
        .map_err(|_| ClientError::Protocol("invalid provider inventory binding"))?;
    let native_endpoint = match bind_native_inventory(
        &inventory,
        client.identity(),
        &client.identity().service_id,
        NativeEndpointSelector::UniqueNative,
        NativeSessionView::Stored,
    ) {
        Ok(binding) => Some(binding.endpoint),
        Err(
            NativeBindingRejection::EndpointUnavailable | NativeBindingRejection::AmbiguousEndpoint,
        ) => None,
        Err(_) => return Err(ClientError::Protocol("invalid native inventory binding")),
    };
    let mut partitions = inventory
        .endpoints
        .iter()
        .filter(|entry| {
            entry
                .channels
                .iter()
                .any(|channel| matches!(channel, ChannelDescription::ExternalProvider { .. }))
        })
        .cloned()
        .map(ProviderReadPartition::ManagedStored)
        .collect::<Vec<_>>();
    if let Some(endpoint) = &binding.native_claude_endpoint {
        let description = inventory
            .endpoints
            .iter()
            .find(|entry| entry.endpoint == *endpoint)
            .ok_or(ClientError::Protocol("native membership unavailable"))?;
        for view in [NativeCatalogueView::Stored, NativeCatalogueView::Loaded] {
            partitions.push(ProviderReadPartition::NativeClaude(
                description.clone(),
                view,
            ));
        }
    }
    let mut rows: Vec<SessionPickerRecord> = Vec::new();
    let mut coverage = Vec::new();
    for partition in partitions {
        let (description, view) = match partition {
            ProviderReadPartition::ManagedStored(description) => (description, None),
            ProviderReadPartition::NativeClaude(description, view) => (description, Some(view)),
        };
        let native = view.is_some();
        let mut partition_seen = BTreeSet::new();
        let mut cursor = None;
        let mut cursors = BTreeSet::new();
        let mut state = NativeLiveReadState::Reading;
        loop {
            let response = timeout_at(
                deadline,
                client.list_provider_sessions(ProviderSessionListParams {
                    endpoint: description.endpoint.clone(),
                    view: view.map_or(NativeSessionView::Stored, NativeCatalogueView::request_view),
                    scope: NativeSessionScope::Any,
                    source: if native {
                        NativeSessionSource::Interactive
                    } else {
                        NativeSessionSource::All
                    },
                    query: None,
                    page_size: 100,
                    cursor,
                }),
            )
            .await
            .unwrap_or(Err(ClientError::Timeout));
            let page = match response {
                Ok(page) => page,
                Err(error) if native => match InventoryReadFailure::from_client(&error) {
                    InventoryReadFailure::Unavailable(reason) => {
                        state = NativeLiveReadState::Unavailable(reason);
                        break;
                    }
                    failure @ InventoryReadFailure::Invalid { .. } => {
                        return Err(failure.into_client());
                    }
                },
                Err(error) => return Err(InventoryReadFailure::from_client(&error).into_client()),
            };
            if page.endpoint != description.endpoint
                || page.sessions.len() > 100
                || page.skipped_records.unwrap_or(0) > 0
            {
                return Err(ClientError::Protocol("invalid provider inventory page"));
            }
            if let Some(view) = view {
                let observed = NativeLiveReadState::from_observation(view, page.native_coverage)
                    .map_err(ClientError::Protocol)?;
                if !matches!(state, NativeLiveReadState::Reading) && state != observed {
                    return Err(ClientError::Protocol(
                        "native coverage changed across pages",
                    ));
                }
                state = observed;
            }
            for summary in page.sessions {
                if summary.target().endpoint != description.endpoint
                    || native != summary.is_native_claude()
                    || view.is_some_and(|view| {
                        !SessionPickerRecord::validate_native_summary(&summary, view)
                    })
                {
                    return Err(ClientError::Protocol("invalid provider inventory origin"));
                }
                let mut row = SessionPickerRecord::from_provider_summary(
                    &summary,
                    &String::from(description.label.clone()),
                )
                .with_provider_endpoint(&summary, &description);
                if let Some(view) = view {
                    row = row.with_native_inventory_view(view);
                }
                if rows.len() >= MAX_RUNTIME_ROWS || !partition_seen.insert(row.identity.clone()) {
                    return Err(ClientError::Protocol(
                        "duplicate or excessive provider inventory",
                    ));
                }
                if let Some(stored) = rows
                    .iter_mut()
                    .find(|stored| stored.identity == row.identity)
                {
                    stored
                        .corroborate_native_loaded(row)
                        .map_err(ClientError::Protocol)?;
                } else {
                    rows.push(row);
                }
            }
            match page.next_cursor {
                Some(next)
                    if !next.is_empty()
                        && next.len() <= 1024
                        && cursors.len() < 64
                        && cursors.insert(next.clone()) =>
                {
                    cursor = Some(next)
                }
                Some(_) => {
                    return Err(ClientError::Protocol(
                        "provider picker cursor did not converge",
                    ));
                }
                None => break,
            }
        }
        if let Some(view) = view {
            coverage.push(NativeLiveCoverage {
                view,
                endpoint: description.endpoint,
                service_epoch: binding.service_epoch.clone(),
                state,
            });
        }
    }
    // Extra live timeout cannot erase previously validated Stored rows. If time
    // remains, a changed source still invalidates the entire captured result.
    if Instant::now() < deadline {
        match timeout_at(deadline, client.list_endpoints()).await {
            Ok(Ok(current)) => {
                let current = SourceInventoryBinding::from_inventory(&binding.service_id, &current)
                    .map_err(|_| ClientError::Protocol("invalid provider inventory binding"))?;
                if current != binding {
                    return Err(ClientError::Protocol("provider inventory binding changed"));
                }
            }
            Ok(Err(error)) => {
                let failure = InventoryReadFailure::from_client(&error);
                let already_partial = coverage.iter().any(|part| part.state.is_incomplete());
                if !already_partial || !matches!(failure, InventoryReadFailure::Unavailable(_)) {
                    return Err(failure.into_client());
                }
            }
            Err(_) => {
                if coverage.is_empty() {
                    return Err(ClientError::Timeout);
                }
                for part in &mut coverage {
                    part.state =
                        NativeLiveReadState::Unavailable(NativeLiveUnavailableReason::TimedOut);
                }
            }
        }
    }
    Ok((native_endpoint, rows, coverage))
}
