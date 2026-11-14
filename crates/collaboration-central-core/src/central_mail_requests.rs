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
        let identity = match &caller.caller {
            m::Caller::Session(session) => m::Identity::AgentSession {
                session: session.clone(),
            },
            m::Caller::Person { person_id, .. } => m::Identity::Person {
                person_id: person_id.clone(),
            },
        };
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
            let actor = self.resolve_identity(&identity).await?;
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
            // A committed request may have lost its alarm publication before its reply.
            let repair = async {
                self.validated_timers().await?;
                central_mail_store::applied(
                    self.host
                        .run_batch(StatementBatch {
                            statements: vec![],
                            alarm: central_mail_store::alarm_update(),
                        })
                        .await?,
                )
            }
            .await;
            match repair {
                Ok(()) => {}
                Err(StoreFailure::Unavailable | StoreFailure::AnswerLost) => {
                    return Ok(Result::OutcomeUnknown);
                }
                Err(error @ StoreFailure::Corrupt { .. }) => return Err(error),
            }
            return Ok(answer);
        }
        if let Some(reason) = crate::central_mail_time::refusal(&request.operation, now) {
            return Ok(Result::Refused(reason));
        }
        let actor = self.resolve_identity(&identity).await?;
        let decision = if let mail::CentralMailOperation::SetNoticeCadence { cadence } =
            &request.operation
        {
            if !matches!(caller.caller, m::Caller::Session(_)) {
                Err(Refusal::NotSession)
            } else {
                let encoded =
                    serde_json::to_value(cadence).map_err(|_| corrupt("notice cadence"))?;
                let cadence_text = encoded.as_str().ok_or_else(|| corrupt("notice cadence"))?;
                let mut statements = vec![crate::guarded_statements::exact(
                    "UPDATE actor_identities SET notice_cadence=?,updated_at=? WHERE actor_identity_id=?",
                    vec![
                        crate::guarded_statements::text(cadence_text),
                        crate::guarded_statements::text(central_mail_store::timestamp(now)),
                        crate::guarded_statements::text(&actor),
                    ],
                )];
                // Plan the new deadline from the existing first activity and last delivery;
                // the actual policy and deadline writes share this request's ledger batch.
                for window in self
                    .notification_windows()
                    .await?
                    .into_iter()
                    .filter(|window| window.recipient == actor)
                {
                    let due = self
                        .notice_due_with_cadence(&actor, window.first, *cadence)
                        .await?;
                    statements.push(crate::guarded_statements::exact(
                        "UPDATE notification_windows SET due_at=?,updated_at=? WHERE window_id=?",
                        vec![
                            crate::guarded_statements::text(central_mail_store::timestamp(due)),
                            crate::guarded_statements::text(central_mail_store::timestamp(now)),
                            crate::guarded_statements::text(&window.id),
                        ],
                    ));
                }
                Ok((
                    statements,
                    mail::CentralMailAnswer::NoticeCadenceSet { cadence: *cadence },
                ))
            }
        } else if let mail::CentralMailOperation::SendMessage { to, body } = &request.operation {
            if !self.known_session(to).await? {
                Err(Refusal::UnknownSession { target: to.clone() })
            } else {
                let message = self.prepare_message_record(
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
