//! Fixed fault cuts borrow the actual writer and leave the persisted schema unchanged.
use crate::{InteractionStore, StorageError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MailStoreTestFault {
    BeforeOutboxInsertion,
    BeforeCarrierReceiptPersistence,
}

impl InteractionStore {
    /// Install only on the owning connection after normal schema admission.
    pub async fn runtime_test_install_mail_fault(
        &self,
        fault: MailStoreTestFault,
    ) -> Result<(), StorageError> {
        let statement = match fault {
            MailStoreTestFault::BeforeOutboxInsertion => {
                "CREATE TEMP TRIGGER rule_before_outbox BEFORE INSERT ON main._fleet_sync_outbox_items BEGIN SELECT RAISE(ABORT,'rule fixture before outbox insertion'); END;"
            }
            MailStoreTestFault::BeforeCarrierReceiptPersistence => {
                "CREATE TEMP TRIGGER rule_before_carrier_receipt BEFORE UPDATE ON main._fleet_sync_steps WHEN NEW.step_name='deliver' AND NEW.kind='call_done' BEGIN SELECT RAISE(ABORT,'rule fixture before carrier receipt persistence'); END;"
            }
        };
        let mut writer = self.writer.lock().await;
        sqlx::raw_sql(statement).execute(&mut *writer).await?;
        Ok(())
    }
}
