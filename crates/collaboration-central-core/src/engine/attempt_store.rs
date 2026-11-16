use super::{
    attempt_records::{EngineSubject, StoredAttempt, decode_attempt},
    attempt_statements::read_attempt,
};
use crate::ObjectHost;
use central_storage::StoreFailure;

/// Keep storage refusal separate from a readable but corrupt control record.
pub(crate) async fn load_attempt<Host: ObjectHost>(
    host: &Host,
    subject: &EngineSubject,
) -> Result<Option<StoredAttempt>, StoreFailure> {
    let rows = host.read(read_attempt(subject)).await?;
    decode_attempt(&rows)
}
