//! An agent update observed by a run that has ended changes nothing of a
//! newer run (design note §11.3; Lead V3): neither its settings catalog nor
//! its capability report.

use super::update_live_settings;
use crate::provider_session_runtime_handles::ProviderSessionRuntimeHandles;
use crate::session_cell_bookkeeping::{admit_create, end_run_and_load_next_for_test};
use crate::session_cell_table::SessionCellTable;
use crate::{
    EventSinkClosed, HistoryReplayFuture, ProviderCapabilityReport, ProviderSettingsCatalog,
    SessionEventSink,
};
use agent_client_protocol::schema::v1::{CurrentModeUpdate, SessionUpdate};
use std::sync::Arc;

const SESSION: &str = "session-a";

struct DiscardingSink;

impl SessionEventSink for DiscardingSink {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }
    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }
    fn publish(
        &self,
        _session_id: &str,
        _event: session_event_model::SessionEvent,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

/// Oracle: Lead V3, a late prompt observation: run A's mode update, read
/// after A ended and run B started, leaves B's catalog and report as B wrote
/// them.
#[tokio::test]
async fn an_ended_runs_live_settings_update_changes_nothing_of_the_newer_run() {
    // Arrange
    let cells = Arc::new(SessionCellTable::default());
    let state =
        ProviderSessionRuntimeHandles::state_for_test(Arc::clone(&cells), Arc::new(DiscardingSink));
    let first = admit_create(&cells, SESSION).expect("created");
    let ended = ProviderSessionRuntimeHandles::for_test(Arc::clone(&cells), &state, first);
    let second = end_run_and_load_next_for_test(&cells, SESSION, first);
    let _catalog = state.session_settings.write(
        SESSION,
        second,
        ProviderSettingsCatalog {
            current_mode: Some("plan".to_owned()),
            ..ProviderSettingsCatalog::default()
        },
    );
    let _report =
        state
            .session_capabilities
            .write(SESSION, second, ProviderCapabilityReport::default());

    // Act
    let observed = update_live_settings(
        &SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new("auto")),
        &ended,
        SESSION,
    );

    // Assert
    assert!(observed.is_ok(), "{observed:?}");
    assert_eq!(
        state
            .session_settings
            .read(SESSION)
            .and_then(|catalog| catalog.current_mode),
        Some("plan".to_owned())
    );
    assert_eq!(
        state
            .session_capabilities
            .read(SESSION)
            .map(|report| report.supports_modes),
        Some(false)
    );
}
