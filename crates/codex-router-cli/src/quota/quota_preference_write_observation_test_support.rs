//! A scoped observer follows the existing adapter into its retained writer; it never injects a store.
use agent_router_quota_tui::QuotaGroupFacet;
use serde::{Serialize, Serializer};
use std::{
    future::Future,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum PreferenceWriteResult {
    Saved,
    Failed,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreferenceWriteFact {
    pub(crate) ordinal: usize,
    #[serde(serialize_with = "serialize_facet")]
    pub(crate) facet: QuotaGroupFacet,
    pub(crate) result: PreferenceWriteResult,
}
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "Serde serialize_with requires a borrowed field"
)]
fn serialize_facet<T: Serializer>(
    facet: &QuotaGroupFacet,
    serializer: T,
) -> Result<T::Ok, T::Error> {
    serializer.serialize_str(facet.stored_value())
}

type WriteSink = Arc<dyn Fn(&PreferenceWriteFact) -> Result<(), String> + Send + Sync>;
#[derive(Default)]
struct ObservedWrites {
    facts: Vec<PreferenceWriteFact>,
    receipt_errors: Vec<String>,
}
#[derive(Clone, Default)]
pub(crate) struct PreferenceWriteObserver {
    writes: Arc<Mutex<ObservedWrites>>,
    sink: Option<WriteSink>,
}
tokio::task_local! { static ACTIVE_PREFERENCE_WRITE_OBSERVER: PreferenceWriteObserver; }
impl PreferenceWriteObserver {
    pub(crate) fn with_sink(sink: WriteSink) -> Self {
        Self {
            sink: Some(sink),
            ..Default::default()
        }
    }
    pub(crate) async fn scope<T>(&self, future: impl Future<Output = T>) -> T {
        ACTIVE_PREFERENCE_WRITE_OBSERVER
            .scope(self.clone(), future)
            .await
    }
    pub(crate) fn facts(&self) -> Result<Vec<PreferenceWriteFact>, String> {
        Ok(self
            .writes
            .lock()
            .map_err(|_| "preference write ledger poisoned")?
            .facts
            .clone())
    }
    pub(crate) fn validated_facts(&self) -> Result<Vec<PreferenceWriteFact>, String> {
        let writes = self
            .writes
            .lock()
            .map_err(|_| "preference write ledger poisoned")?;
        if !writes.receipt_errors.is_empty() {
            return Err(format!(
                "preference write receipt failed: {:?}",
                writes.receipt_errors
            ));
        }
        Ok(writes.facts.clone())
    }
    pub(crate) fn save_returned(
        &self,
        facet: QuotaGroupFacet,
        result: Result<(), ui_preference_store::UiPreferenceSaveError>,
    ) {
        let fact = {
            let Ok(mut writes) = self.writes.lock() else {
                return;
            };
            let fact = PreferenceWriteFact {
                ordinal: writes.facts.len() + 1,
                facet,
                result: if result.is_ok() {
                    PreferenceWriteResult::Saved
                } else {
                    PreferenceWriteResult::Failed
                },
            };
            writes.facts.push(fact.clone());
            fact
        };
        if let Some(sink) = &self.sink
            && let Err(error) = sink(&fact)
            && let Ok(mut writes) = self.writes.lock()
        {
            writes.receipt_errors.push(error);
        }
    }
}
pub(crate) fn current() -> Option<PreferenceWriteObserver> {
    ACTIVE_PREFERENCE_WRITE_OBSERVER.try_with(Clone::clone).ok()
}

#[path = "quota_preference_write_observer_contract_test.rs"]
mod tests;
