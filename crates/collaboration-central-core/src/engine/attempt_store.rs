use super::{
    attempt_records::{EngineSubject, StoredAttempt, decode_attempt},
    attempt_statements::read_attempt,
};
use fleet_runtime_ports::CentralStore;
use central_storage::StoreFailure;

/// Keep storage refusal separate from a readable but corrupt control record.
pub(crate) async fn load_attempt<Host: CentralStore>(
    host: &Host,
    subject: &EngineSubject,
) -> Result<Option<StoredAttempt>, StoreFailure> {
    let rows = host.read(read_attempt(subject)).await?;
    decode_attempt(&rows)
}
