//! Invocation-local alias ownership after every configured profile has observed its binding.
use crate::session_picker::SourceContinuation;
use crate::session_picker::SourceInventoryResult;
use crate::session_picker::source::source_reload_worker::BTreeMap;
use crate::session_picker::source::source_reload_worker::BoundSourceSnapshots;
use crate::session_picker::source::source_reload_worker::PickerRecordsSnapshot;
use crate::session_picker::source::source_reload_worker::PickerSourceContext;
use crate::session_picker::source::source_reload_worker::SourceInventoryBinding;
use crate::session_picker::source::source_reload_worker::SourceInventoryRejection;
use crate::session_picker::source::source_reload_worker::SourceInventoryRequest;
use crate::session_picker::source::source_reload_worker::SourceReadProgress;
use crate::session_picker::source::source_reload_worker::SourceReadState;
use crate::session_picker::source::source_reload_worker::VecDeque;

pub(crate) struct QualifiedSourceRead {
    request: SourceInventoryRequest,
    binding: SourceInventoryBinding,
    continuation: SourceContinuation,
}

impl QualifiedSourceRead {
    pub(crate) fn from_result(
        expected: &SourceInventoryRequest,
        result: SourceInventoryResult,
    ) -> Result<Self, SourceInventoryRejection> {
        let SourceInventoryResult::Qualified {
            request,
            binding,
            continuation,
        } = result
        else {
            return Err(SourceInventoryRejection::InvalidInventory);
        };
        if request != *expected {
            return Err(SourceInventoryRejection::StaleSnapshot);
        }
        if expected.continuation.is_some() {
            return Err(SourceInventoryRejection::InvalidInventory);
        }
        let PickerSourceContext::ConfiguredHosted(profile) = &request.source_context else {
            return Err(SourceInventoryRejection::InvalidInventory);
        };
        if profile.service_id != binding.service_id {
            return Err(SourceInventoryRejection::InvalidInventory);
        }
        Ok(Self {
            request,
            binding,
            continuation,
        })
    }
}

#[derive(Default)]
pub(crate) struct SourceAliasGroups {
    groups: Vec<QualifiedAliasGroup>,
}

struct QualifiedAliasGroup {
    binding: SourceInventoryBinding,
    representative: PickerSourceContext,
    aliases: Vec<PickerSourceContext>,
}

impl SourceAliasGroups {
    pub(crate) fn extend(&mut self, observed: Self) {
        self.groups.extend(observed.groups);
    }

    pub(crate) fn take_settled_qualifications(
        qualified: &mut Vec<QualifiedSourceRead>,
        pending: &BTreeMap<collaboration_client::protocol::UuidIdentity, usize>,
    ) -> Vec<QualifiedSourceRead> {
        let mut ready = Vec::new();
        let mut retained = Vec::new();
        for observed in qualified.drain(..) {
            if pending.get(&observed.binding.service_id) == Some(&0) {
                ready.push(observed);
            } else {
                retained.push(observed);
            }
        }
        *qualified = retained;
        ready
    }

    pub(crate) fn prepare(
        source_order: &[PickerSourceContext],
        mut qualified: Vec<QualifiedSourceRead>,
        progress: &mut [SourceReadProgress],
    ) -> (
        Self,
        VecDeque<SourceInventoryRequest>,
        Option<SourceInventoryRejection>,
    ) {
        let mut groups = Self::default();
        let mut queued = VecDeque::new();
        let mut rejection = None;
        for source in source_order {
            let Some(index) = qualified
                .iter()
                .position(|entry| entry.request.source_context == *source)
            else {
                continue;
            };
            let observed = qualified.remove(index);
            let conflicts = qualified.iter().any(|other| {
                other.binding.service_id == observed.binding.service_id
                    && other.binding != observed.binding
            });
            if conflicts {
                let mut rejected = vec![observed.request.source_context];
                qualified.retain(|other| {
                    if other.binding.service_id == observed.binding.service_id {
                        rejected.push(other.request.source_context.clone());
                        false
                    } else {
                        true
                    }
                });
                for item in progress
                    .iter_mut()
                    .filter(|item| rejected.contains(&item.source_context))
                {
                    item.read_state = SourceReadState::Rejected {
                        reason: SourceInventoryRejection::StaleSnapshot,
                    };
                }
                rejection = Some(SourceInventoryRejection::StaleSnapshot);
                continue;
            }
            let mut aliases = vec![observed.request.source_context.clone()];
            qualified.retain(|other| {
                if other.binding == observed.binding {
                    aliases.push(other.request.source_context.clone());
                    false
                } else {
                    true
                }
            });
            groups.groups.push(QualifiedAliasGroup {
                binding: observed.binding,
                representative: observed.request.source_context.clone(),
                aliases,
            });
            let mut request = observed.request;
            request.continuation = Some(observed.continuation);
            queued.push_back(request);
        }
        (groups, queued, rejection)
    }

    pub(crate) fn synchronize(
        &self,
        source: &PickerSourceContext,
        snapshots: &mut BoundSourceSnapshots,
        progress: &mut [SourceReadProgress],
    ) {
        let Some(group) = self
            .groups
            .iter()
            .find(|group| group.representative == *source)
        else {
            return;
        };
        let state = progress
            .iter()
            .find(|item| item.source_context == *source)
            .map(|item| item.read_state.clone());
        let snapshot = snapshots
            .iter()
            .find(|(context, _, _)| context == source)
            .map(|(_, binding, snapshot)| {
                (
                    binding.clone(),
                    snapshot.runtime_coverage,
                    snapshot.native_live_coverage.clone(),
                )
            });
        for alias in &group.aliases {
            if let Some(item) = progress
                .iter_mut()
                .find(|item| item.source_context == *alias)
                && let Some(state) = &state
            {
                item.read_state = state.clone();
            }
            if alias == source {
                continue;
            }
            snapshots.retain(|(context, _, _)| context != alias);
            if let Some((binding, coverage, native_coverage)) = &snapshot
                && binding.as_ref() == Some(&group.binding)
            {
                // The representative owns the rows; aliases contribute only verified labels.
                snapshots.push((
                    alias.clone(),
                    binding.clone(),
                    PickerRecordsSnapshot {
                        native_live_coverage: native_coverage.clone(),
                        records: Vec::new(),
                        runtime_coverage: *coverage,
                    },
                ));
            }
        }
    }
}
