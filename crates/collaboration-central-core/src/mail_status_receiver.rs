//! Mailbox publications append atomically in the recipient's delivery leaf.

use crate::{
    CallerKind, DeliveryKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority, object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork, session_store::CoreSessionStore,
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{
    AtomicStore, CallFailure, JournalScope, ReborrowScope, StoreError, storage::TransactionScope,
};
use fleet_sync_delivery::{
    DeliveryError, PublicationTarget, PublishCall, PublishStatus, StatusStream,
};
use fleet_sync_protocol::{StatusPublication, StatusPublicationResult};
use runtime_primitives::Clock;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<DeliveryKind, Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn mail_publication(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Mailbox
            || !matches!(invocation.source, ObjectAddress::Mailbox(_))
            || !matches!(invocation.authority, CallAuthority::Object)
        {
            return Err(CallFailure::AnswerLost);
        }
        let ObjectWork::Session {
            host,
            shared_host,
            replies,
            installation,
            ..
        } = &self.work
        else {
            return Err(CallFailure::AnswerLost);
        };
        let target: PublishCall<BoardDeliveryTypes> =
            serde_json::from_value(invocation.target).map_err(|_| CallFailure::AnswerLost)?;
        let publication: StatusPublication<_> =
            serde_json::from_value(invocation.request).map_err(|_| CallFailure::AnswerLost)?;
        let PublicationTarget::StatusStream { sender, route } = target.target else {
            return encode(crate::mail_status_dispatch::invalid_route());
        };
        if &route != installation {
            return encode(crate::mail_status_dispatch::invalid_route());
        }
        let store = Arc::new(CoreSessionStore::new(
            host.clone(),
            shared_host.clone(),
            replies.clone(),
        ));
        let now = shared_host.now();
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    match StatusStream::<_, BoardDeliveryTypes>::new(scope, sender, route)
                        .append_and_admit::<PublishStatus<
                            CoreSessionStore<Host, Io>,
                            Host::Database,
                            BoardDeliveryTypes,
                        >>(&publication)
                        .await
                    {
                        Ok(_) => {
                            scope.ensure_wake(now).await?;
                            Ok(StatusPublicationResult::Applied)
                        }
                        Err(DeliveryError::Refused { reason }) => {
                            Ok(StatusPublicationResult::Refused { reason })
                        }
                        Err(error) => Err(StoreError::backend(error)),
                    }
                })
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)
            .and_then(encode)
    }
}

fn encode(result: StatusPublicationResult) -> Result<Vec<u8>, CallFailure> {
    serde_json::to_vec(&result).map_err(|_| CallFailure::AnswerLost)
}
