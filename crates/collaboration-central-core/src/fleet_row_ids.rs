//! Fleet writes draw validated row IDs from their runtime's injected capability.

use durable_steps::{StoreError, Uuid};
use runtime_primitives::IdSource;
use std::sync::Arc;

pub(crate) struct FleetRowIds(Arc<dyn IdSource>);

impl FleetRowIds {
    pub fn new(source: Arc<dyn IdSource>) -> Self {
        Self(source)
    }

    pub fn next(&mut self) -> Result<Uuid, StoreError> {
        let id = self.0.new_id();
        validate_row_id(id)?;
        Ok(id)
    }
}

pub(crate) fn validate_row_id(id: Uuid) -> Result<(), StoreError> {
    if !runtime_primitives::uuid_v7(id) {
        return Err(StoreError::Aborted {
            reason: "the injected ID source must supply UUIDv7 row identifiers".into(),
        });
    }
    Ok(())
}
