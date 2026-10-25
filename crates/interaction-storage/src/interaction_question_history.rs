//! Question validation and history mutations within the shared owning transaction.
use super::*;

impl InteractionHistoryStore {
    pub async fn record_question(
        &self,
        requester: SessionRef,
        approver: Identity,
        request: QuestionRequest,
    ) -> Result<(), InteractionHistoryError> {
        if matches!(&approver, Identity::Session { session } if session == &requester) {
            return Err(InteractionHistoryError::SelfApprover);
        }
        let record = InteractionHistoryRecord::Question {
            requester,
            approver,
            request,
            state: QuestionHistoryState::Pending,
        };
        if !record.is_valid_stored_value() {
            return Err(InteractionHistoryError::InvalidQuestion);
        }
        self.mutate_history(|records| {
            if records.contains_key(record.request_id()) {
                return Err(InteractionHistoryError::AlreadyExists);
            }
            let mut next = records.clone();
            next.insert_with_timestamp(record.request_id().to_owned(), record, Utc::now());
            Ok((next, ()))
        })
        .await
    }

    pub async fn record_cancelled_question(
        &self,
        requester: SessionRef,
        approver: Identity,
        request: QuestionRequest,
        reason: &str,
    ) -> Result<(), InteractionHistoryError> {
        let record = InteractionHistoryRecord::Question {
            requester,
            approver,
            request,
            state: QuestionHistoryState::Cancelled {
                reason: reason.to_owned().into(),
            },
        };
        if !record.is_valid_stored_value() {
            return Err(InteractionHistoryError::InvalidQuestion);
        }
        self.mutate_history(|records| {
            if records.contains_key(record.request_id()) {
                return Err(InteractionHistoryError::AlreadyExists);
            }
            let mut next = records.clone();
            next.insert_with_timestamp(record.request_id().to_owned(), record, Utc::now());
            Ok((next, ()))
        })
        .await
    }

    pub async fn list_questions(&self, pending_only: bool) -> Vec<InteractionHistoryRecord> {
        self.data.lock().await.values()
            .filter(|record| matches!(record, InteractionHistoryRecord::Question { state, .. } if !pending_only || state == &QuestionHistoryState::Pending))
            .cloned().collect()
    }

    pub async fn respond_question(
        &self,
        request_id: &str,
        actor: &Identity,
        response: &QuestionResponse,
    ) -> Result<(), InteractionHistoryError> {
        self.mutate_history(|records| {
            let record = records
                .get(request_id)
                .ok_or(InteractionHistoryError::NotPending)?;
            validate_question_response_record(record, actor, response)?;
            let mut next = records.clone();
            let Some(InteractionHistoryRecord::Question { state, .. }) = next.get_mut(request_id)
            else {
                return Err(InteractionHistoryError::NotPending);
            };
            *state = match response {
                QuestionResponse::Answered { content } => QuestionHistoryState::Answered {
                    content: content.clone(),
                },
                QuestionResponse::Declined => QuestionHistoryState::Declined,
                QuestionResponse::Cancelled => QuestionHistoryState::Cancelled {
                    reason: InteractionCancelReason::ApproverCancelled,
                },
            };
            Ok((next, ()))
        })
        .await
    }

    pub async fn validate_question_response(
        &self,
        request_id: &str,
        actor: &Identity,
        response: &QuestionResponse,
    ) -> Result<(), InteractionHistoryError> {
        self.mutate_history(|records| {
            let record = records
                .get(request_id)
                .ok_or(InteractionHistoryError::NotPending)?;
            validate_question_response_record(record, actor, response)?;
            Ok((records.clone(), ()))
        })
        .await
    }
    pub async fn cancel_questions(
        &self,
        requester: &SessionRef,
        reason: &str,
    ) -> Result<Vec<String>, InteractionHistoryError> {
        if reason.trim().is_empty() {
            return Err(InteractionHistoryError::Unavailable);
        }
        self.mutate_history(|records| {
            let mut next = records.clone();
            let mut cancelled = Vec::new();
            for (request_id, record) in &mut next.records {
                if let InteractionHistoryRecord::Question {
                    requester: owner,
                    state,
                    ..
                } = record
                    && owner == requester
                    && state == &QuestionHistoryState::Pending
                {
                    *state = QuestionHistoryState::Cancelled {
                        reason: reason.to_owned().into(),
                    };
                    cancelled.push(request_id.clone());
                }
            }
            if cancelled.is_empty() {
                return Ok((records.clone(), cancelled));
            }
            Ok((next, cancelled))
        })
        .await
    }

    pub async fn cancel_question(
        &self,
        request_id: &str,
        reason: &str,
    ) -> Result<(), InteractionHistoryError> {
        if reason.trim().is_empty() {
            return Err(InteractionHistoryError::Unavailable);
        }
        self.mutate_history(|records| {
            let mut next = records.clone();
            let Some(InteractionHistoryRecord::Question { state, .. }) = next.get_mut(request_id)
            else {
                return Err(InteractionHistoryError::NotPending);
            };
            if state != &QuestionHistoryState::Pending {
                return Err(InteractionHistoryError::AlreadySettled);
            }
            *state = QuestionHistoryState::Cancelled {
                reason: reason.to_owned().into(),
            };
            Ok((next, ()))
        })
        .await
    }
}
