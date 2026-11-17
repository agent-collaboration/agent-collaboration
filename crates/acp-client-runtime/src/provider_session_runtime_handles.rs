//! What one session's actor and its projections share with the connection:
//! the event sink scoped to the session's run, the behaviour, the settings
//! and capability catalogs, the session cells and the agent's steering
//! capability. The actor and the prompt observation both read it.

use crate::SessionEventSink;
use crate::provider_connection_activity::ProviderConnectionActivity;
use crate::provider_cursor_plan_items::CursorPlanItems;
use crate::run_owned_state::RunOwnedMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct ProviderSessionRuntimeHandles {
    // event_sink: for one session's actor, a SessionGenerationSink scoped to
    // the generation it was admitted under (for_session_generation).
    pub(crate) connection_model_reasoning: crate::ModelReasoningMetadata,
    pub(crate) behavior: Arc<dyn crate::ProviderBehavior>,
    pub(crate) event_sink: Arc<dyn SessionEventSink>,
    pub(crate) sink_closed: CancellationToken,
    pub(crate) tool_registry: Arc<ProviderConnectionActivity>,
    pub(crate) todo_state: Arc<CursorPlanItems>,
    /// Each run's settings catalog and capability report (design note
    /// §11.3): written only under the owning run's generation.
    pub(crate) session_settings: Arc<RunOwnedMap<crate::ProviderSettingsCatalog>>,
    pub(crate) session_capabilities: Arc<RunOwnedMap<crate::ProviderCapabilityReport>>,
    /// In-folder grants, so a confirmed mode change can turn auto's grant on or off.
    pub(crate) in_folder_grants: crate::in_folder_allowance::InFolderGrants,
    /// Sessions whose caller wants fast kept off (D1 R9). It records the wish;
    /// the session cell's fast setting records whether it's applied.
    pub(crate) keep_fast_off: KeepFastOff,
    /// The connection's session cells (D1 state machines).
    pub(crate) session_cells: Arc<crate::session_cell_table::SessionCellTable>,
    /// The run these handles serve; untracked until for_session_generation.
    pub(crate) run_generation: provider_session_machines::SessionGeneration,
    /// The agent advertises steering, so a busy refusal offers it (D1 M8).
    pub(crate) agent_steers: bool,
    /// Where an actor that ends on its own hands its run's end: the
    /// connection loop, which moves the cell and publishes Unloaded in one
    /// iteration (Lead V1). Unbounded, so an exiting actor never waits on it.
    pub(crate) actor_exits: tokio::sync::mpsc::UnboundedSender<ActorExit>,
}

#[cfg(test)]
impl ProviderSessionRuntimeHandles {
    /// One run's handles over `cells`, with fresh run-owned state shared
    /// through `state` (tests build an ended run's and a newer run's from it).
    pub(crate) fn for_test(
        cells: Arc<crate::session_cell_table::SessionCellTable>,
        state: &Self,
        generation: provider_session_machines::SessionGeneration,
    ) -> Self {
        let mut handles = state.clone();
        handles.session_cells = cells;
        handles.run_generation = generation;
        handles
    }

    /// Shared run-owned state over `cells`, publishing into `event_sink`.
    pub(crate) fn state_for_test(
        cells: Arc<crate::session_cell_table::SessionCellTable>,
        event_sink: Arc<dyn SessionEventSink>,
    ) -> Self {
        Self {
            connection_model_reasoning: crate::ModelReasoningMetadata::default(),
            behavior: Arc::new(crate::test_behavior::TestProviderBehavior),
            event_sink,
            sink_closed: CancellationToken::new(),
            tool_registry: Arc::new(ProviderConnectionActivity::default()),
            todo_state: Arc::new(CursorPlanItems::default()),
            session_settings: Arc::new(RunOwnedMap::new(Arc::clone(&cells))),
            session_capabilities: Arc::new(RunOwnedMap::new(Arc::clone(&cells))),
            in_folder_grants: Arc::new(RunOwnedMap::new(Arc::clone(&cells))),
            keep_fast_off: Arc::new(RunOwnedMap::new(Arc::clone(&cells))),
            session_cells: cells,
            run_generation: provider_session_machines::SessionGeneration::UNTRACKED,
            agent_steers: false,
            actor_exits: tokio::sync::mpsc::unbounded_channel().0,
        }
    }
}

/// A session's actor ended on its own, ending run `generation`.
pub(crate) struct ActorExit {
    pub(crate) provider_session_id: String,
    pub(crate) generation: provider_session_machines::SessionGeneration,
}

impl ProviderSessionRuntimeHandles {
    /// These handles for one session admitted under `generation`: its
    /// actor, projections and settings work publish only into that run.
    pub(crate) fn for_session_generation(
        &self,
        generation: provider_session_machines::SessionGeneration,
    ) -> Self {
        let mut handles = self.clone();
        handles.event_sink = Arc::new(crate::session_event_sink::SessionGenerationSink::new(
            Arc::clone(&self.event_sink),
            generation,
        ));
        handles.run_generation = generation;
        handles
    }
}

/// The runs whose caller wants fast kept off (D1 R9): present means kept off.
pub(crate) type KeepFastOff = Arc<RunOwnedMap<()>>;
