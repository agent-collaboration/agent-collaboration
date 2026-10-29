//! Read-only completion observation; opening or driving an object is not observation.
use super::{NativeObjects, ObjectAddress, StoreError, address_key};

impl NativeObjects {
    pub(crate) async fn await_startup_alarm(
        &self,
        address: ObjectAddress,
    ) -> Result<(), StoreError> {
        let key = address_key(&address)?;
        let slot = self
            .slots
            .lock()
            .await
            .get(&key)
            .cloned()
            .ok_or_else(|| StoreError::backend("startup alarm has no discovered object"))?;
        let mut completed = slot.startup_alarm_completed.subscribe();
        completed
            .wait_for(|completed| *completed)
            .await
            .map_err(StoreError::backend)?;
        Ok(())
    }
}
