//! Run-local grouping survives view remounts; its retained writer serializes optional saves.
use crate::QuotaGroupFacet;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::watch;

pub type QuotaPreferenceFuture<'a, TValue> = Pin<Box<dyn Future<Output = TValue> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuotaPreferenceSaveError;

pub trait QuotaUiPreferenceStore: Send + Sync {
    fn load_group_facet(&self) -> QuotaPreferenceFuture<'_, QuotaGroupFacet>;
    fn save_group_facet(
        &self,
        facet: QuotaGroupFacet,
    ) -> QuotaPreferenceFuture<'_, Result<(), QuotaPreferenceSaveError>>;
}

#[derive(Clone)]
pub struct QuotaGroupPreferences {
    choices: watch::Sender<PreferenceWriterState>,
}

pub struct QuotaGroupPreferenceWriter {
    store: Arc<dyn QuotaUiPreferenceStore>,
    choices: watch::Receiver<PreferenceWriterState>,
    initial_facet: QuotaGroupFacet,
}

impl QuotaGroupPreferences {
    pub async fn load(
        store: Arc<dyn QuotaUiPreferenceStore>,
    ) -> (Self, QuotaGroupPreferenceWriter) {
        let initial = store.load_group_facet().await;
        let (choices, receiver) = watch::channel(PreferenceWriterState::Running(initial));
        (
            Self { choices },
            QuotaGroupPreferenceWriter {
                store,
                choices: receiver,
                initial_facet: initial,
            },
        )
    }

    pub fn current_facet(&self) -> QuotaGroupFacet {
        self.choices.borrow().facet()
    }

    pub fn remember(&self, facet: QuotaGroupFacet) {
        self.choices.send_if_modified(|current| match current {
            PreferenceWriterState::Running(previous) if *previous != facet => {
                *previous = facet;
                true
            }
            _ => false,
        });
    }
    pub fn finish(&self) {
        self.choices.send_if_modified(|current| match *current {
            PreferenceWriterState::Running(facet) => {
                *current = PreferenceWriterState::Finished(facet);
                true
            }
            PreferenceWriterState::Finished(_) => false,
        });
    }
}

#[derive(Clone, Copy)]
enum PreferenceWriterState {
    Running(QuotaGroupFacet),
    Finished(QuotaGroupFacet),
}
impl PreferenceWriterState {
    fn facet(self) -> QuotaGroupFacet {
        match self {
            Self::Running(facet) | Self::Finished(facet) => facet,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuotaPreferenceWriterStopped;
impl std::fmt::Display for QuotaPreferenceWriterStopped {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("quota preference writer stopped without its terminal lifecycle signal")
    }
}
impl std::error::Error for QuotaPreferenceWriterStopped {}

impl QuotaGroupPreferenceWriter {
    /// The owner signals finish after its loop; stale snapshot handles cannot hold the drain open.
    pub async fn run(mut self) -> Result<(), QuotaPreferenceWriterStopped> {
        let mut last_attempted = self.initial_facet;
        loop {
            self.choices
                .changed()
                .await
                .map_err(|_| QuotaPreferenceWriterStopped)?;
            let next = *self.choices.borrow_and_update();
            let facet = next.facet();
            if facet != last_attempted {
                last_attempted = facet;
                let _ = self.store.save_group_facet(facet).await;
            }
            if matches!(next, PreferenceWriterState::Finished(_)) {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
#[path = "quota_group_preferences_test.rs"]
mod tests;
