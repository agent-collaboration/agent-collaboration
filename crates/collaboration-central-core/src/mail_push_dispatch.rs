//! Resolve Fleet custody, then await a recipient's bounded socket reply outside storage.
use crate::{
    ObjectHost, PlatformIo,
    holder_reply_hub::HolderReplyHub,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallId, CallRetryReason, StoreError,
    storage::TransactionScope,
};
use fleet_sync_delivery::{DeliveryLimits, Outbox, PushCall};
use fleet_sync_protocol::{
    ClientAnswer, DeliveryEnvelope, DeliveryItemRef, DeliveryTypes, PushResult, ServerCall,
    ServerCallAnswer,
};
use futures_util::future::{Either, select};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{
    convert::Infallible,
    sync::{Arc, Weak},
    time::Duration,
};

type PushAnswer = PushResult<<BoardDeliveryTypes as DeliveryTypes>::Acceptance>;
pub(crate) type PushDispatcher = Arc<
    dyn Fn(
            PushCall<BoardDeliveryTypes>,
            DeliveryItemRef,
            CallId,
        ) -> BoxFuture<'static, Result<PushAnswer, CallFailure>>
        + Send
        + Sync,
>;

pub(crate) fn bind<Host, Io>(
    host: Weak<Host>,
    io: Arc<Io>,
    replies: Arc<HolderReplyHub>,
    limits: DeliveryLimits,
    timeout: Duration,
) -> PushDispatcher
where
    Host: ObjectHost,
    Io: PlatformIo,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    Arc::new(move |target, reference, call| {
        let host = host.clone();
        let io = io.clone();
        let replies = replies.clone();
        Box::pin(async move {
            let host = host.upgrade().ok_or(CallFailure::AnswerLost)?;
            let (item, _) = StepDriver::new(host.clone())
                .run(ResolvePush { reference, limits })
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
            let item = item.ok_or(CallFailure::AnswerLost)?;
            if item.recipient().peer != target.recipient {
                return Err(CallFailure::AnswerLost);
            }
            let waiter = crate::peer_request::begin_peer_request(
                host,
                io.clone(),
                replies,
                target.recipient,
                ServerCall::Push { call, item: *item },
                timeout,
            )
            .await?;
            let remaining = Duration::from_millis(
                u64::try_from(
                    waiter
                        .deadline()
                        .unix_millis()
                        .saturating_sub(io.now().unix_millis()),
                )
                .unwrap_or(0),
            );
            match select(
                Box::pin(waiter.answer()),
                Box::pin(io.reply_deadline(remaining)),
            )
            .await
            {
                Either::Left((Ok(ClientAnswer::Crate(ServerCallAnswer::Pushed(answer))), _)) => {
                    PushCall::<BoardDeliveryTypes>::validated_answer(answer)
                }
                Either::Left((Err(error), _)) => Err(error),
                Either::Left((Ok(_), _)) => Err(CallFailure::AnswerLost),
                Either::Right(_) => Err(CallFailure::Retryable {
                    reason: CallRetryReason {
                        code: "pushDeadline".into(),
                        detail: "recipient Push answer deadline elapsed".into(),
                    },
                }),
            }
        })
    })
}

struct ResolvePush {
    reference: DeliveryItemRef,
    limits: DeliveryLimits,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ResolvePush
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<Box<DeliveryEnvelope<BoardDeliveryTypes>>>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let item = Outbox::<_, BoardDeliveryTypes>::new(scope, self.limits)
                .resolve_push(&self.reference)
                .await
                .map_err(StoreError::backend)?;
            Ok((item.map(Box::new), Vec::new()))
        })
    }
}
