//! Central-only admitted request ledger; cached answers precede mutable decisions.
use crate::{
    ObjectHost, WorkspaceCaller, WorkspaceRuntime, WorkspaceStoreCalls,
    central_mail_store::{self, insert_message},
    workspace_decode_error::corrupt,
    workspace_row_codec::{json, string},
};
use central_storage::{BatchOutcome, StatementBatch, StoreFailure};
use collaboration_protocol::central_mail as mail;
use workspace_model as m;
impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub async fn request_central(
        &self,
        caller: &WorkspaceCaller,
        request: mail::CentralMailRequest,
    ) -> Result<mail::CentralMailResult, StoreFailure> {
        match self.execute_central(caller, request).await {
            Err(StoreFailure::Unavailable | StoreFailure::AnswerLost) => {
                Ok(mail::CentralMailResult::CentralUnavailable)
            }
            result => result,
        }
    }
    async fn execute_central(
        &self,
        caller: &WorkspaceCaller,
        request: mail::CentralMailRequest,
    ) -> Result<mail::CentralMailResult, StoreFailure> {
        use mail::{CentralMailRefusal as Refusal, CentralMailResult as Result};
        if let Err(reason) = self.admit_workspace(caller, None).await? {
            return Ok(Result::Refused(Refusal::Workspace(reason)));
        }
        if let Some(field) = crate::central_mail_time::refusal(&request.operation, self.now()?) {
            return Ok(Result::Refused(Refusal::TimeOutOfRange { field }));
        }
        let identity = match &caller.caller {
            m::Caller::Session(session) => m::Identity::AgentSession {
                session: session.clone(),
            },
            m::Caller::Person { person_id, .. } => m::Identity::Person {
                person_id: person_id.clone(),
            },
        };
        let actor = self.resolve_identity(&identity).await?;
        let digest = crate::workspace_request_log::digest(&request.operation)?;
        let _turn = self.take_central_turn().await;
        let now = self.now()?;
        let cutoff = (now - chrono::Duration::hours(24)).to_rfc3339();
        let rows = self
            .host
            .read(crate::workspace_request_log::lookup(
                &request.request_id,
                &cutoff,
            ))
            .await?;
        if let Some(row) = rows.rows.first() {
            if string(&rows, row, "caller")? != actor.as_str()
                || string(&rows, row, "content_digest")? != digest
            {
                return Ok(Result::Refused(Refusal::Workspace(
                    m::RefusalReason::RequestIdReused,
                )));
            }
            let answer: Result = json(string(&rows, row, "answer")?, "request_log.answer")?;
            if matches!(
                answer,
                Result::Conflict | Result::OutcomeUnknown | Result::CentralUnavailable
            ) {
                return Err(corrupt("request_log.answer"));
            }
            return Ok(answer);
        }
        let decision =
            if let mail::CentralMailOperation::SendMessage { to, body } = &request.operation {
                if !self.known_session(to).await? {
                    Err(Refusal::UnknownSession { target: to.clone() })
                } else {
                    let message = self.queued_message(
                        mail::MessageSender::Actor(actor.clone()),
                        to.clone(),
                        mail::MessageBody::Direct { body: body.clone() },
                        None,
                        now,
                    )?;
                    Ok((
                        vec![insert_message(&message)?],
                        mail::CentralMailAnswer::Queued {
                            message_id: message.message_id,
                        },
                    ))
                }
            } else {
                self.plan_timer_request(&request.operation, now).await?
            };
        let (mut statements, answer) = match decision {
            Ok((statements, answer)) => (statements, Result::Done(answer)),
            Err(reason) => (vec![], Result::Refused(reason)),
        };
        self.validated_timers().await?;
        statements.extend(crate::workspace_request_log::retain(
            &request.request_id,
            &actor,
            &digest,
            &answer,
            &now.to_rfc3339(),
            &cutoff,
        )?);
        match self
            .host
            .run_batch(StatementBatch {
                statements,
                alarm: central_mail_store::alarm_update(),
            })
            .await
        {
            Ok(BatchOutcome::Applied { .. }) => {}
            Ok(_) => return Ok(Result::Conflict),
            Err(StoreFailure::AnswerLost) => return Ok(Result::OutcomeUnknown),
            Err(StoreFailure::Unavailable) => return Ok(Result::CentralUnavailable),
            Err(error) => return Err(error),
        }
        #[cfg(feature = "test-support")]
        if self.take_lost_reply(&request.request_id) {
            return Ok(Result::OutcomeUnknown);
        }
        Ok(answer)
    }
}
