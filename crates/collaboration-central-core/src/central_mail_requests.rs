//! Central-only admitted request ledger; cached answers precede mutable decisions.
use crate::{
    WorkspaceCaller, WorkspaceRuntime, central_mail_store,
    workspace_decode_error::corrupt,
    workspace_row_codec::{json, string},
};
use central_storage::{BatchOutcome, StatementBatch, StoreFailure};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_model as m;
use workspace_model::central_mail as mail;
impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub async fn request_central(
        &self,
        caller: &WorkspaceCaller,
        request: mail::CentralMailRequest,
    ) -> Result<mail::CentralMailResult, StoreFailure> {
        // The request path answers as on main; the due batch that follows checks
        // the organization stamp once for this event before any unit (P3).
        let result = self.execute_central(caller, request).await;
        // Due work and publication cannot replace this event's durable answer.
        let _ = self.after_event().await;
        match result {
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
        let _turn = self.take_central_turn().await;
        if let Err(reason) = self.admit_workspace(caller, None).await? {
            return Ok(Result::Refused(Refusal::Workspace(reason)));
        }
        let identity = match caller.caller() {
            m::Caller::Session(session) => m::Identity::AgentSession {
                session: session.clone(),
            },
            m::Caller::Person { person_id, .. } => m::Identity::Person {
                person_id: person_id.clone(),
            },
        };
        let digest = crate::workspace_request_log::digest(&request.operation)?;
        let now = self.now()?;
        let cutoff = (now - chrono::Duration::hours(24)).to_rfc3339();
        let rows = self
            .host
            .read(crate::workspace_request_log::lookup(
                crate::workspace_request_log::RequestLogStore::Organization,
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
            let answer: Result = json(string(&rows, row, "answer")?, "runtime_request_log.answer")?;
            if matches!(
                answer,
                Result::Conflict | Result::OutcomeUnknown | Result::CentralUnavailable
            ) {
                return Err(corrupt("runtime_request_log.answer"));
            }
            // A committed request may have lost its alarm publication before its reply.
            let repair = async {
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
                Err(
                    error
                    @ (StoreFailure::Corrupt { .. } | StoreFailure::AlarmUnpublishable { .. }),
                ) => return Err(error),
            }
            return Ok(answer);
        }
        if let Some(reason) = crate::central_mail_time::refusal(&request.operation, now) {
            return Ok(Result::Refused(reason));
        }
        let actor = self.resolve_identity(&identity).await?;
        let mut delivery_ready = None;
        let decision = if let mail::CentralMailOperation::SetNoticeCadence { cadence } =
            &request.operation
        {
            if !matches!(caller.caller(), m::Caller::Session(_)) {
                Err(Refusal::NotSession)
            } else {
                let encoded =
                    serde_json::to_value(cadence).map_err(|_| corrupt("notice cadence"))?;
                let cadence_text = encoded.as_str().ok_or_else(|| corrupt("notice cadence"))?;
                let mut statements = vec![crate::guarded_statements::exact(
                    "UPDATE access_actor_identities SET notice_cadence=?,updated_at=? WHERE actor_identity_id=?",
                    vec![
                        crate::guarded_statements::text(cadence_text),
                        crate::guarded_statements::text(central_mail_store::timestamp(now)),
                        crate::guarded_statements::text(&actor),
                    ],
                )];
                // Recompute the pending deadline from now on the new epoch cadence;
                // the actual policy and deadline writes share this request's ledger batch.
                for window in self
                    .notification_windows()
                    .await?
                    .into_iter()
                    .filter(|window| window.recipient == actor)
                {
                    let due = Self::notice_due_with_cadence(now, *cadence)?;
                    statements.push(crate::guarded_statements::exact(
                        "UPDATE mail_notification_windows SET due_at=?,updated_at=? WHERE window_id=?",
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
        } else if let mail::CentralMailOperation::SendMessage {
            to,
            body,
            delivery,
            reply_to,
            sent_at,
        } = &request.operation
        {
            if !self.known_session(to).await? {
                Err(Refusal::UnknownSession { target: to.clone() })
            } else {
                let message = self.prepare_message_record(
                    mail::MessageSender::Actor(actor.clone()),
                    to.clone(),
                    mail::MessageBody::Direct {
                        body: body.clone(),
                        delivery: *delivery,
                        reply_to: reply_to.clone(),
                    },
                    None,
                    now,
                )?;
                delivery_ready = Some(crate::MailDeliveryReady::from(&message));
                Ok((
                    vec![crate::central_mail_rows::insert_message_due(
                        &message,
                        Some(&crate::central_mail_rows::DirectMessageTerms {
                            cancel_due_at: crate::message_deadline::cancel_due_at(*sent_at, now),
                            delivery_window_ends_at:
                                crate::message_deadline::delivery_window_ends_at(*sent_at, now),
                            sender_request_id: request.request_id.clone(),
                        }),
                    )?],
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

        statements.extend(crate::workspace_request_log::retain(
            crate::workspace_request_log::RequestLogStore::Organization,
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
            // A rejected statement keeps main's store-outage answer on this path.
            Ok(BatchOutcome::StatementRejected { .. }) => return Ok(Result::CentralUnavailable),
            Ok(_) => return Ok(Result::Conflict),
            Err(StoreFailure::AnswerLost) => return Ok(Result::OutcomeUnknown),
            Err(StoreFailure::Unavailable) => return Ok(Result::CentralUnavailable),
            Err(error) => return Err(error),
        }
        if let Some(delivery) = delivery_ready {
            (self.mail_delivery_ready)(delivery);
        }
        #[cfg(feature = "test-support")]
        if self.take_lost_reply(&request.request_id) {
            return Ok(Result::OutcomeUnknown);
        }
        Ok(answer)
    }
}
