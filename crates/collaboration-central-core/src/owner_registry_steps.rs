//! Capture and replay native transfer admission inside one borrowed Registry step.
use crate::{
    generated_enrollment,
    machine_enrollment::identity_from_stored_enrollment,
    owner_registry_calls::*,
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, JournalScope, StartRefusal, StoreError, storage::TransactionScope,
};
use fleet_sync_lease::{DatabaseLifecycle, HolderChangeTask};
use fleet_sync_protocol::{
    ActorRef, DatabaseId, HolderChangeAnswer, HolderChangeRefusal, HolderChangeRequest, PeerId,
};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct PrepareOwnerTransfer {
    pub database: DatabaseId,
    pub offer: OwnerTransferOffer,
    pub now: SyncTime,
}
pub(crate) enum PreparedOwnerTransfer {
    Admitted(HolderChangeRequest),
    Refused(OwnerTransferAnswer),
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for PrepareOwnerTransfer
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = PreparedOwnerTransfer;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let Some(target) = target_peer(scope, self.offer.installation()).await? else {
                return Ok((
                    PreparedOwnerTransfer::Refused(OwnerTransferAnswer::Refused(
                        HolderChangeRefusal::NotFound.into(),
                    )),
                    Vec::new(),
                ));
            };
            let attempt = self.offer.attempt();
            let saved = fleet_sync_lease::holder_change_view(scope, attempt)
                .await
                .map_err(StoreError::backend)?
                .map(|view| view.request);
            let request = match self.offer {
                OwnerTransferOffer::Move(offered) => {
                    let from_epoch = match &saved {
                        Some(HolderChangeRequest::Move { from_epoch, .. }) => *from_epoch,
                        Some(_) => {
                            return Ok((
                                PreparedOwnerTransfer::Refused(
                                    OwnerTransferAnswer::ConflictingRequest,
                                ),
                                Vec::new(),
                            ));
                        }
                        None => {
                            match fleet_sync_lease::lease_status(scope, self.database, self.now)
                                .await
                            {
                                Ok(Some(lease)) => lease.epoch,
                                Ok(None) => {
                                    return Ok((
                                        PreparedOwnerTransfer::Refused(
                                            OwnerTransferAnswer::Refused(
                                                HolderChangeRefusal::NotFound.into(),
                                            ),
                                        ),
                                        Vec::new(),
                                    ));
                                }
                                Err(fleet_sync_lease::LeaseError::Retired) => {
                                    return Ok((
                                        PreparedOwnerTransfer::Refused(
                                            OwnerTransferAnswer::Refused(
                                                HolderChangeRefusal::Retired.into(),
                                            ),
                                        ),
                                        Vec::new(),
                                    ));
                                }
                                Err(error) => return Err(StoreError::backend(error)),
                            }
                        }
                    };
                    HolderChangeRequest::Move {
                        attempt,
                        database: self.database,
                        from_epoch,
                        to: target,
                        reason: offered.reason,
                    }
                }
                OwnerTransferOffer::Takeover(offered) => HolderChangeRequest::Takeover {
                    attempt,
                    database: self.database,
                    preview: offered.preview,
                    to: target,
                    reason: offered.reason,
                },
            };
            if saved.is_some_and(|saved| saved != request) {
                return Ok((
                    PreparedOwnerTransfer::Refused(OwnerTransferAnswer::ConflictingRequest),
                    Vec::new(),
                ));
            }
            let key = fleet_sync_lease::holder_change_request_key(attempt)
                .map_err(StoreError::backend)?;
            // The public lease admission erases StartRefusal into LeaseError text.
            // Check the same idempotent journal admission first to preserve a
            // typed conflict, including an ID used by another task kind.
            match scope
                .admit::<HolderChangeTask<Store, Driver>>(&key, &request)
                .await
            {
                Ok(_) => {}
                Err(StartRefusal::RequestConflict { .. }) => {
                    return Ok((
                        PreparedOwnerTransfer::Refused(OwnerTransferAnswer::ConflictingRequest),
                        Vec::new(),
                    ));
                }
                Err(StartRefusal::Storage(error)) => return Err(error),
                Err(StartRefusal::UnknownKind) => {
                    return Err(StoreError::backend("holder-change task unavailable"));
                }
            }
            fleet_sync_lease::admit_holder_change::<Store, Driver>(
                scope,
                &request,
                &ActorRef::new("owner").map_err(StoreError::backend)?,
                self.now,
            )
            .await
            .map_err(StoreError::backend)?;
            Ok((PreparedOwnerTransfer::Admitted(request), Vec::new()))
        })
    }
}

async fn target_peer<Scope: TransactionScope>(
    scope: &mut Scope,
    installation: &message_board::ServiceId,
) -> Result<Option<PeerId>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut rows = generated_enrollment::read_enrollments(
        scope,
        Some(installation.as_str().to_owned()),
        None,
        None,
        None,
    )
    .await
    .map_err(StoreError::backend)?;
    if rows.len() > 1 {
        return Err(StoreError::backend("duplicate installation identity"));
    }
    let Some(row) = rows.pop() else {
        return Ok(None);
    };
    let identity = identity_from_stored_enrollment(row)?;
    if &identity.origin.installation != installation {
        return Err(StoreError::backend("installation identity mismatch"));
    }
    Ok(Some(identity.peer))
}

pub(crate) struct ReadOwnerTransfer {
    pub request: HolderChangeRequest,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadOwnerTransfer
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = OwnerTransferAnswer;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let (attempt, moving) = match &self.request {
                HolderChangeRequest::Move { attempt, .. } => (*attempt, true),
                HolderChangeRequest::Takeover { attempt, .. } => (*attempt, false),
                HolderChangeRequest::Claim { .. } => {
                    return Err(StoreError::backend("owner transfer cannot claim"));
                }
            };
            // A canonical answer preserves a grant even when later work failed.
            let answer = fleet_sync_lease::holder_change_answer(scope, &self.request)
                .await
                .map_err(StoreError::backend)?;
            let reply = match answer {
                Some(HolderChangeAnswer::Granted { epoch, start, .. }) if moving => {
                    OwnerTransferAnswer::Moved {
                        new_epoch: epoch,
                        start,
                    }
                }
                Some(HolderChangeAnswer::Granted { epoch, start, .. }) => {
                    OwnerTransferAnswer::Granted {
                        new_epoch: epoch,
                        start,
                    }
                }
                Some(HolderChangeAnswer::Refused(reason)) => {
                    OwnerTransferAnswer::Refused(reason.into())
                }
                Some(HolderChangeAnswer::Cancelled { by }) => OwnerTransferAnswer::Cancelled { by },
                Some(HolderChangeAnswer::Failed { reason }) => {
                    OwnerTransferAnswer::Failed { reason }
                }
                Some(HolderChangeAnswer::Pending { .. }) | None => {
                    let view = fleet_sync_lease::holder_change_view(scope, attempt)
                        .await
                        .map_err(StoreError::backend)?;
                    match view
                        .and_then(|view| view.waiting)
                        .and_then(|waiting| waiting.copy_unavailable)
                    {
                        Some(reason) => OwnerTransferAnswer::Unavailable {
                            subject: OwnerBlockedSubject::Transfer { transfer: attempt },
                            reason,
                        },
                        None => OwnerTransferAnswer::Pending {
                            transfer_id: attempt,
                        },
                    }
                }
            };
            Ok((reply, Vec::new()))
        })
    }
}

pub(crate) struct CheckOwnerPreview {
    pub database: DatabaseId,
    pub now: SyncTime,
}
pub(crate) enum OwnerPreviewCheck {
    Held,
    Refused(OwnerPreviewAnswer),
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for CheckOwnerPreview
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = OwnerPreviewCheck;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let checked = match fleet_sync_lease::database_lifecycle(scope, self.database)
                .await
                .map_err(StoreError::backend)?
            {
                None => OwnerPreviewCheck::Refused(OwnerPreviewAnswer::NotFound),
                Some(DatabaseLifecycle::Retired { .. } | DatabaseLifecycle::Retiring { .. }) => {
                    OwnerPreviewCheck::Refused(OwnerPreviewAnswer::Retired)
                }
                Some(_) => {
                    match fleet_sync_lease::lease_status(scope, self.database, self.now).await {
                        Ok(Some(_)) => OwnerPreviewCheck::Held,
                        Ok(None) => OwnerPreviewCheck::Refused(OwnerPreviewAnswer::NotFound),
                        Err(fleet_sync_lease::LeaseError::Retired) => {
                            OwnerPreviewCheck::Refused(OwnerPreviewAnswer::Retired)
                        }
                        Err(error) => return Err(StoreError::backend(error)),
                    }
                }
            };
            Ok((checked, Vec::new()))
        })
    }
}

pub(crate) struct ReadOwnerPreviewDisplay {
    pub preview: fleet_sync_protocol::TakeoverPreview,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadOwnerPreviewDisplay
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = OwnerPreviewSuccess;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let mut rows = generated_enrollment::read_enrollments(
                scope,
                None,
                None,
                None,
                Some(self.preview.holder.to_string()),
            )
            .await
            .map_err(StoreError::backend)?;
            if rows.len() != 1 {
                return Err(StoreError::backend(
                    "preview holder has no unique enrollment identity",
                ));
            }
            let identity = identity_from_stored_enrollment(
                rows.pop()
                    .ok_or_else(|| StoreError::backend("preview enrollment disappeared"))?,
            )?;
            if identity.peer != self.preview.holder {
                return Err(StoreError::backend("preview holder enrollment differs"));
            }
            Ok((
                OwnerPreviewSuccess {
                    preview: self.preview,
                    holder_installation: identity.origin.installation,
                    last_seen: identity.last_seen,
                },
                Vec::new(),
            ))
        })
    }
}
