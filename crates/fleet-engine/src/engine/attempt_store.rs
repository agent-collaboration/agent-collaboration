use super::{
    attempt_records::{StoredAttempt, decode_attempt},
    attempt_statements::read_attempt,
};
use central_storage::StoreFailure;
use fleet_contracts::EngineSubject;
use fleet_runtime_ports::CentralStore;

/// Keep storage refusal separate from a readable but corrupt control record.
pub(crate) async fn load_attempt<Host: CentralStore>(
    host: &Host,
    subject: &EngineSubject,
) -> Result<Option<StoredAttempt>, StoreFailure> {
    let rows = host.read(read_attempt(subject)).await?;
    decode_attempt(&rows)
}
