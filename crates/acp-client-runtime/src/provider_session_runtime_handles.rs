//! What one session's actor and its projections share with the connection:
//! the event sink scoped to the session's run, the behaviour, the settings
//! and capability catalogs, the session cells and the agent's steering
//! capability. The actor and the prompt observation both read it.

use crate::SessionEventSink;
use crate::provider_connection_activity::ProviderConnectionActivity;
use crate::provider_cursor_plan_items::CursorPlanItems;
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
    pub(crate) session_settings:
        Arc<tokio::sync::RwLock<std::collections::HashMap<String, crate::ProviderSettingsCatalog>>>,
    pub(crate) session_capabilities: Arc<
        tokio::sync::RwLock<std::collections::HashMap<String, crate::ProviderCapabilityReport>>,
    >,
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

/// Provider session ids the Router keeps non-fast, for this Router's life.
pub(crate) type KeepFastOff = Arc<tokio::sync::RwLock<std::collections::HashSet<String>>>;
