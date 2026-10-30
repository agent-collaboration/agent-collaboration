//! One bounded source page; the picker owns accumulation, scheduling and publication.
use super::{
    SessionPickerIdentity, SessionPickerRecord, SessionsSource, configured_inventory_query,
    configured_inventory_transport::ConfiguredInventoryClient,
    picker_runtime_inventory::native_inventory_pager::NativeInventoryPager,
    router_connection_registry::RouterConnectionProfile,
};
use crate::{
    picker_runtime_status::{
        NativeCatalogueView, NativeLiveCoverage, NativeLiveReadState, NativeLiveUnavailableReason,
        PickerRecordsSnapshot, PickerRuntimeCoverage,
    },
    source_inventory_binding::{InventoryEndpointKind, SourceInventoryBinding},
    source_inventory_request::{
        InventoryPageProgress, PickerSourceContext, SourceContinuation, SourceInventoryRejection,
        SourceInventoryRequest, SourceInventoryResult,
    },
};
use collaboration_client::{
    ClientError,
    protocol::{
        EndpointInventory, EndpointRef, NativeSessionListParams, NativeSessionScope,
        NativeSessionSource, NativeSessionView, ProviderSessionListParams,
    },
};
use std::{
    collections::{BTreeSet, VecDeque},
    time::Duration,
};

const MAX_SOURCE_ROWS: usize = 4096;
const MAX_SOURCE_PAGES: usize = 65;
const SOURCE_READ_DEADLINE: Duration = Duration::from_secs(30);

pub struct StoredSourceContinuation {
    captured: SourceInventoryRequest,
    binding: SourceInventoryBinding,
    inventory: EndpointInventory,
    client: ConfiguredInventoryClient,
    pending: VecDeque<StoredReadPartition>,
    native_pager: Option<NativeInventoryPager>,
    provider_cursor: Option<String>,
    provider_cursors: BTreeSet<String>,
    provider_pages: usize,
    seen: BTreeSet<SessionPickerIdentity>,
    native_live_coverage: Vec<NativeLiveCoverage>,
    partition_seen: BTreeSet<SessionPickerIdentity>,
}

#[derive(Clone)]
enum StoredReadPartition {
    NativeCodex(EndpointRef),
    ManagedStored(EndpointRef),
    NativeClaude(EndpointRef, NativeCatalogueView),
}
impl StoredReadPartition {
    fn endpoint(&self) -> &EndpointRef {
        match self {
            Self::NativeCodex(endpoint)
            | Self::ManagedStored(endpoint)
            | Self::NativeClaude(endpoint, _) => endpoint,
        }
    }
    fn is_native_claude(&self) -> bool {
        matches!(self, Self::NativeClaude(..))
    }
}

enum ConfiguredReadOutcome {
    Qualified(SourceInventoryBinding, SourceContinuation),
    Page(
        SourceInventoryBinding,
        PickerRecordsSnapshot,
        InventoryPageProgress,
    ),
}

pub async fn load(request: SourceInventoryRequest) -> SourceInventoryResult {
    let deadline = tokio::time::Instant::now() + SOURCE_READ_DEADLINE;
    let result = read_page(&request, deadline).await;
    match result {
        Ok(ConfiguredReadOutcome::Qualified(binding, continuation)) => {
            SourceInventoryResult::Qualified {
                request,
                binding,
                continuation,
            }
        }
        Ok(ConfiguredReadOutcome::Page(binding, snapshot, progress)) => {
            SourceInventoryResult::Ready {
                request,
                binding: Some(binding),
                snapshot,
                progress,
            }
        }
        Err(reason) => SourceInventoryResult::Rejected { request, reason },
    }
}

async fn read_page(
    request: &SourceInventoryRequest,
    deadline: tokio::time::Instant,
) -> Result<ConfiguredReadOutcome, SourceInventoryRejection> {
    let PickerSourceContext::ConfiguredHosted(profile) = &request.source_context else {
        return Err(SourceInventoryRejection::InvalidInventory);
    };
    configured_inventory_query::qualify_configured_query(&request.query)?;
    if profile.has_unqualified_http_credential() {
        return Err(SourceInventoryRejection::EndpointUnqualified);
    }
    let mut captured = request.clone();
    captured.continuation = None;
    let mut state = if let Some(cursor) = &request.continuation {
        let state = cursor
            .take()
            .await
            .ok_or(SourceInventoryRejection::InvalidInventory)?;
        if state.captured != captured {
            return Err(SourceInventoryRejection::StaleSnapshot);
        }
        state
    } else {
        let state =
            tokio::time::timeout_at(deadline, StoredSourceContinuation::start(profile, captured))
                .await
                .map_err(|_| SourceInventoryRejection::SourceUnavailable)??;
        return Ok(ConfiguredReadOutcome::Qualified(
            state.binding.clone(),
            SourceContinuation::stored(state),
        ));
    };
    let native_view = match state.pending.front() {
        Some(StoredReadPartition::NativeClaude(_, view)) => Some(*view),
        _ => None,
    };
    let native_read = native_view.is_some();
    let rows = state.next_page(deadline).await?;
    // Revalidate every page when the read budget permits. An unavailable extra
    // native read may retain its previously qualified prefix, never an invalid reply.
    match tokio::time::timeout_at(deadline, state.client.inventory()).await {
        Ok(Ok(inventory)) => {
            let final_binding =
                SourceInventoryBinding::from_inventory(&profile.service_id, &inventory)?;
            if final_binding != state.binding {
                return Err(SourceInventoryRejection::StaleSnapshot);
            }
        }
        Ok(Err(error)) => {
            let availability = matches!(
                super::source_inventory_failure::InventoryReadFailure::from_client(&error),
                super::source_inventory_failure::InventoryReadFailure::Unavailable(_)
            );
            let already_partial = state
                .native_live_coverage
                .iter()
                .any(|part| part.state.is_incomplete());
            if !native_read || !availability || !already_partial {
                return Err(read_rejection(error));
            }
        }
        Err(_) if native_read => {
            state.mark_native_unavailable(native_view, NativeLiveUnavailableReason::TimedOut)
        }
        Err(_) => return Err(SourceInventoryRejection::SourceUnavailable),
    }
    let native_live_coverage = state.native_live_coverage.clone();
    let binding = state.binding.clone();
    let progress = if state.pending.is_empty() {
        InventoryPageProgress::Complete
    } else {
        InventoryPageProgress::MoreAvailable {
            continuation: SourceContinuation::stored(state),
        }
    };
    Ok(ConfiguredReadOutcome::Page(
        binding,
        PickerRecordsSnapshot {
            native_live_coverage,
            records: rows,
            runtime_coverage: PickerRuntimeCoverage::Unavailable,
        },
        progress,
    ))
}

impl StoredSourceContinuation {
    async fn start(
        profile: &RouterConnectionProfile,
        captured: SourceInventoryRequest,
    ) -> Result<Self, SourceInventoryRejection> {
        let client = ConfiguredInventoryClient::new(profile).map_err(read_rejection)?;
        let inventory = client.inventory().await.map_err(read_rejection)?;
        let binding = SourceInventoryBinding::from_inventory(&profile.service_id, &inventory)?;
        configured_inventory_query::qualify_bound_source_filter(&captured.query, &binding)?;
        let mut pending: VecDeque<_> = binding
            .endpoints
            .iter()
            .map(|(endpoint, kind)| match kind {
                InventoryEndpointKind::NativeCodex => {
                    StoredReadPartition::NativeCodex(endpoint.clone())
                }
                InventoryEndpointKind::ExternalProvider => {
                    StoredReadPartition::ManagedStored(endpoint.clone())
                }
            })
            .collect();
        let native_live_coverage = if let Some(endpoint) = &binding.native_claude_endpoint {
            [NativeCatalogueView::Stored, NativeCatalogueView::Loaded]
                .into_iter()
                .map(|view| {
                    pending.push_back(StoredReadPartition::NativeClaude(endpoint.clone(), view));
                    NativeLiveCoverage {
                        view,
                        endpoint: endpoint.clone(),
                        service_epoch: binding.service_epoch.clone(),
                        state: NativeLiveReadState::Reading,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(Self {
            native_live_coverage,
            captured,
            binding,
            inventory,
            client,
            pending,
            native_pager: None,
            provider_cursor: None,
            provider_cursors: BTreeSet::new(),
            provider_pages: 0,
            seen: BTreeSet::new(),
            partition_seen: BTreeSet::new(),
        })
    }

    async fn next_page(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<Vec<SessionPickerRecord>, SourceInventoryRejection> {
        let partition = self
            .pending
            .front()
            .cloned()
            .ok_or(SourceInventoryRejection::InvalidInventory)?;
        let endpoint = partition.endpoint().clone();
        let native = partition.is_native_claude();
        let view = match &partition {
            StoredReadPartition::NativeClaude(_, view) => Some(*view),
            _ => None,
        };
        let source = match self.captured.query.source {
            SessionsSource::All => NativeSessionSource::All,
            SessionsSource::Interactive => NativeSessionSource::Interactive,
            SessionsSource::Subagents => NativeSessionSource::Subagents,
        };
        let (mut rows, complete) = match partition {
            StoredReadPartition::NativeCodex(_) => {
                if self.native_pager.is_none() {
                    self.native_pager = Some(
                        NativeInventoryPager::new(
                            NativeSessionListParams {
                                endpoint,
                                view: NativeSessionView::Stored,
                                scope: NativeSessionScope::Any,
                                source,
                                include_empty_sessions: self.captured.query.include_empty_sessions,
                                query: None,
                                page_size: 100,
                                cursor: None,
                            },
                            None,
                        )
                        .map_err(read_rejection)?,
                    );
                }
                let page_future = self
                    .native_pager
                    .as_mut()
                    .ok_or(SourceInventoryRejection::InvalidInventory)?
                    .next_page(&mut self.client);
                let page = tokio::time::timeout_at(deadline, page_future)
                    .await
                    .map_err(|_| SourceInventoryRejection::SourceUnavailable)?
                    .map_err(read_rejection)?
                    .ok_or(SourceInventoryRejection::InvalidInventory)?;
                let complete = page.next_cursor.is_none();
                let mut rows = Vec::new();
                for summary in page.sessions {
                    self.accept_target(SessionPickerIdentity::HostedCodex(summary.target.clone()))?;
                    rows.push(SessionPickerRecord::from_native_summary(&summary));
                }
                (rows, complete)
            }
            StoredReadPartition::ManagedStored(_) | StoredReadPartition::NativeClaude(..) => {
                if self.provider_pages >= MAX_SOURCE_PAGES {
                    return Err(SourceInventoryRejection::InvalidInventory);
                }
                let result = tokio::time::timeout_at(
                    deadline,
                    self.client.provider_page(&ProviderSessionListParams {
                        endpoint: endpoint.clone(),
                        view: view
                            .map_or(NativeSessionView::Stored, NativeCatalogueView::request_view),
                        scope: NativeSessionScope::Any,
                        source: if native {
                            NativeSessionSource::Interactive
                        } else {
                            source
                        },
                        query: None,
                        page_size: 100,
                        cursor: self.provider_cursor.clone(),
                    }),
                )
                .await
                .unwrap_or(Err(
                    super::source_inventory_failure::InventoryReadFailure::Unavailable(
                        NativeLiveUnavailableReason::TimedOut,
                    ),
                ));
                let page = match result {
                    Ok(page) => page,
                    Err(super::source_inventory_failure::InventoryReadFailure::Unavailable(
                        reason,
                    )) if native => {
                        self.mark_native_unavailable(view, reason);
                        self.finish_partition();
                        return Ok(Vec::new());
                    }
                    Err(super::source_inventory_failure::InventoryReadFailure::Unavailable(_)) => {
                        return Err(SourceInventoryRejection::SourceUnavailable);
                    }
                    Err(failure) => return Err(failure.into_rejection()),
                };
                self.provider_pages += 1;
                if page.endpoint != endpoint
                    || page.sessions.len() > 100
                    || page.skipped_records.unwrap_or(0) > 0
                {
                    return Err(SourceInventoryRejection::InvalidInventory);
                }
                if let Some(view) = view {
                    if page.native_coverage.as_ref().is_some_and(|coverage| {
                        coverage
                            .conflicts
                            .iter()
                            .any(|conflict| conflict.target.endpoint != endpoint)
                    }) {
                        return Err(SourceInventoryRejection::InvalidInventory);
                    }
                    let observed =
                        NativeLiveReadState::from_observation(view, page.native_coverage)
                            .map_err(|_| SourceInventoryRejection::InvalidInventory)?;
                    let current = self
                        .native_live_coverage
                        .iter_mut()
                        .find(|part| part.view == view)
                        .ok_or(SourceInventoryRejection::InvalidInventory)?;
                    if !matches!(current.state, NativeLiveReadState::Reading)
                        && current.state != observed
                    {
                        return Err(SourceInventoryRejection::InvalidInventory);
                    }
                    current.state = observed;
                }
                let description = self
                    .inventory
                    .endpoints
                    .iter()
                    .find(|entry| entry.endpoint == endpoint)
                    .ok_or(SourceInventoryRejection::InvalidInventory)?
                    .clone();
                let mut rows = Vec::new();
                for summary in page.sessions {
                    if summary.target().endpoint != endpoint
                        || native != summary.is_native_claude()
                        || view.is_some_and(|view| {
                            !SessionPickerRecord::validate_native_summary(&summary, view)
                        })
                    {
                        return Err(SourceInventoryRejection::InvalidInventory);
                    }
                    let mut row = SessionPickerRecord::from_source_provider_summary(
                        &summary,
                        &String::from(description.label.clone()),
                    )
                    .with_provider_endpoint(&summary, &description);
                    if let Some(view) = view {
                        row = row.with_native_inventory_view(view);
                    }
                    if !self.partition_seen.insert(row.identity.clone()) {
                        return Err(SourceInventoryRejection::InvalidInventory);
                    }
                    if view != Some(NativeCatalogueView::Loaded)
                        || !self.seen.contains(&row.identity)
                    {
                        self.accept_target(row.identity.clone())?;
                    }
                    rows.push(row);
                }
                let complete = match page.next_cursor {
                    None => true,
                    Some(cursor)
                        if !cursor.is_empty()
                            && cursor.len() <= 1024
                            && self.provider_cursors.insert(cursor.clone()) =>
                    {
                        self.provider_cursor = Some(cursor);
                        false
                    }
                    Some(_) => return Err(SourceInventoryRejection::InvalidInventory),
                };
                (rows, complete)
            }
        };
        if complete {
            self.finish_partition();
        }
        for part in &self.native_live_coverage {
            SessionPickerRecord::apply_native_conflicts(&mut rows, part);
        }
        Ok(rows)
    }

    fn finish_partition(&mut self) {
        self.pending.pop_front();
        self.native_pager = None;
        self.provider_cursor = None;
        self.provider_cursors.clear();
        self.provider_pages = 0;
        self.partition_seen.clear();
    }
    fn mark_native_unavailable(
        &mut self,
        view: Option<NativeCatalogueView>,
        reason: NativeLiveUnavailableReason,
    ) {
        for part in &mut self.native_live_coverage {
            if Some(part.view) == view {
                part.state = NativeLiveReadState::Unavailable(reason);
            }
        }
    }

    fn accept_target(
        &mut self,
        target: SessionPickerIdentity,
    ) -> Result<(), SourceInventoryRejection> {
        if self.seen.len() >= MAX_SOURCE_ROWS || !self.seen.insert(target) {
            return Err(SourceInventoryRejection::InvalidInventory);
        }
        Ok(())
    }
}

fn read_rejection(error: ClientError) -> SourceInventoryRejection {
    match error {
        ClientError::Timeout
        | ClientError::Transport(_)
        | ClientError::Discovery { .. }
        | ClientError::Rejected { .. } => SourceInventoryRejection::SourceUnavailable,
        _ => SourceInventoryRejection::InvalidInventory,
    }
}
