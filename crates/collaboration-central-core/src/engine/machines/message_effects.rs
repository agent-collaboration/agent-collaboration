use central_engine_model::Context;
use workspace_model::central_mail as mail;

/// The id source gave something other than a UUIDv7 for a new message.
#[derive(Debug)]
pub(crate) struct NotUuidV7;
/// A queued message from central, minted with the next id from the context.
pub(crate) fn queue_message(
    context: &mut Context<'_>,
    target: workspace_ids::SessionRef,
    body: mail::MessageBody,
) -> Result<mail::MessageRecord, NotUuidV7> {
    let id = context.ids.next_id();
    if id.get_version_num() != 7 {
        return Err(NotUuidV7);
    }
    Ok(mail::MessageRecord::queued_central(
        id.to_string().try_into().map_err(|_| NotUuidV7)?,
        target,
        body,
        context.now,
    ))
}
