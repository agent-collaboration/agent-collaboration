//! A settings continuation of a run that has ended keeps nothing (design
//! note §11.3; Lead V3): the catalog its agent returned never replaces a
//! newer run's.

use super::keep_returned_catalog;
use crate::provider_session_runtime_handles::ProviderSessionRuntimeHandles;
use crate::session_cell_bookkeeping::{admit_create, end_run_and_load_next_for_test};
use crate::session_cell_table::SessionCellTable;
use crate::{EventSinkClosed, HistoryReplayFuture, ProviderSettingsCatalog, SessionEventSink};
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

fn catalog(mode: &str) -> ProviderSettingsCatalog {
    ProviderSettingsCatalog {
        current_mode: Some(mode.to_owned()),
        ..ProviderSettingsCatalog::default()
    }
}

/// Oracle: Lead V3, the held continuation: a settings change of run A whose
/// agent reply arrives after A ended and run B started keeps its catalog for
/// A only; B's catalog survives.
#[tokio::test]
async fn an_ended_runs_returned_catalog_never_replaces_the_newer_runs() {
    // Arrange
    let cells = Arc::new(SessionCellTable::default());
    let state =
        ProviderSessionRuntimeHandles::state_for_test(Arc::clone(&cells), Arc::new(DiscardingSink));
    let first = admit_create(&cells, SESSION).expect("created");
    let ended = ProviderSessionRuntimeHandles::for_test(Arc::clone(&cells), &state, first);
    let second = end_run_and_load_next_for_test(&cells, SESSION, first);
    let _newer = state
        .session_settings
        .write(SESSION, second, catalog("plan"));

    // Act
    let kept = keep_returned_catalog(&ended, SESSION, &catalog("default"));

    // Assert
    assert!(kept.is_ok(), "{kept:?}");
    assert_eq!(
        state
            .session_settings
            .read(SESSION)
            .and_then(|catalog| catalog.current_mode),
        Some("plan".to_owned())
    );
}
