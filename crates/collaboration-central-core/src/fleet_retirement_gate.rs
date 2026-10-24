//! Retirement admission reads the canonical parent board in Fleet's own step.
use crate::{
    CallerKind, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    fleet_record_codec::corrupt,
    fleet_record_store,
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, CallFailure, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{GateAnswer, GateReason, GateReasonCode, RetirementAdmission};
use message_board::{BoardState, ProjectId, Record};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn retirement_gate_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source != ObjectAddress::Registry
            || call.source_face != CallerKind::Registry
            || !matches!(call.authority, CallAuthority::Object)
        {
            return Err(CallFailure::AnswerLost);
        }
        let _: fleet_sync_lease::RetirementAdmission =
            serde_json::from_value(call.target).map_err(encode_refusal)?;
        let request: RetirementAdmission =
            serde_json::from_value(call.request).map_err(encode_refusal)?;
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        let (answer, _) = StepDriver::new(host.clone())
            .run(ReadRetirementGate(request))
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }
}
struct ReadRetirementGate(RetirementAdmission);
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadRetirementGate
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = GateAnswer;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let project = ProjectId::try_from(self.0.database.to_string())
                .map_err(|error| corrupt("retirement.project", error))?;
            let Some(stored) = fleet_record_store::read(scope, &project.into()).await? else {
                return Ok((
                    refused("not_found", "Retirement project is absent.")?,
                    Vec::new(),
                ));
            };
            let Record::DirectoryEntry(project) = stored.record else {
                return Err(corrupt("retirement.project", "wrong record kind"));
            };
            let parent = fleet_record_store::read(scope, &project.board.into())
                .await?
                .ok_or_else(|| corrupt("retirement.board", "parent board is absent"))?;
            let Record::Board(board) = parent.record else {
                return Err(corrupt("retirement.board", "wrong record kind"));
            };
            let answer = match board.state {
                BoardState::Archived => GateAnswer::Admitted,
                BoardState::Open | BoardState::Closed | BoardState::Archiving => refused(
                    "board_not_archived",
                    "Retirement requires an archived board.",
                )?,
            };
            Ok((answer, Vec::new()))
        })
    }
}
fn refused(code: &str, detail: &str) -> Result<GateAnswer, StoreError> {
    Ok(GateAnswer::Refused {
        reason: GateReason {
            code: GateReasonCode::new(code).map_err(StoreError::backend)?,
            detail: detail.into(),
        },
    })
}
