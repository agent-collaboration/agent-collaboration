//! The public boundary signs raw receipt keysets and rechecks current decisions.
use super::{BoardOperations, admission_error};
use message_board::{
    BoardError, CanonicalHash, RequestDecision, RequestEffectsCursor, RequestEffectsRead,
    RequestEffectsRefusal, RequestEffectsRequest, RequestEffectsScope, RequestId, RequestResult,
    WriteReply, public_replies as public,
};
use message_board_storage::{decode_board_cursor, encode_board_cursor};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReceiptContinuation {
    scope: RequestEffectsScope,
    request: RequestId,
    count: u64,
    digest: CanonicalHash,
    cursor: RequestEffectsCursor,
}

impl BoardOperations<'_> {
    pub async fn request_effects(
        &self,
        request: public::RequestEffectsRequest,
    ) -> Result<public::RequestEffectsRead, BoardError> {
        let continuation = if let Some(after) = &request.after {
            let key = self.receipt_signing_key().await?;
            let decoded: ReceiptContinuation = match decode_board_cursor(key, after) {
                Ok(decoded) => decoded,
                Err(_) => return Ok(invalid_cursor()),
            };
            if decoded.scope != request.scope || decoded.request != request.request {
                return Ok(invalid_cursor());
            }
            Some(decoded)
        } else {
            None
        };
        let result = self
            .admission()?
            .request_effects(&RequestEffectsRequest {
                scope: request.scope.clone(),
                request: request.request.clone(),
                after: continuation.as_ref().map(|token| token.cursor.clone()),
            })
            .await
            .map_err(admission_error)?;
        match result {
            RequestEffectsRead::Refused { reason } => {
                Ok(public::RequestEffectsRead::Refused { reason })
            }
            RequestEffectsRead::Decision { decision } => {
                if let RequestDecision::Accepted { effects, .. } = &decision
                    && continuation.as_ref().is_some_and(|token| {
                        token.count != effects.total_count() || &token.digest != effects.digest()
                    })
                {
                    return Ok(invalid_cursor());
                }
                Ok(public::RequestEffectsRead::Decision {
                    decision: self
                        .public_decision(&request.scope, &request.request, decision)
                        .await?,
                })
            }
        }
    }

    async fn receipt_signing_key(
        &self,
    ) -> Result<&message_board_storage::CursorSigningKey, BoardError> {
        self.admission()?
            .registry
            .cursor_key()
            .await
            .map_err(|error| admission_error(error.into()))
    }

    pub(super) async fn public_write_reply(
        &self,
        scope: RequestEffectsScope,
        request: &RequestId,
        reply: WriteReply,
    ) -> Result<public::WriteReply, BoardError> {
        // Exhaustive conversion makes a new native outcome require a public decision here.
        let result = match reply.result {
            RequestResult::Decided(decision) => public::RequestResult::Decided(
                self.public_decision(&scope, request, decision).await?,
            ),
            RequestResult::ConflictingRequest => public::RequestResult::ConflictingRequest,
            RequestResult::NotWriter { current } => public::RequestResult::NotWriter { current },
            RequestResult::LeaseExpired { expired_at } => {
                public::RequestResult::LeaseExpired { expired_at }
            }
            RequestResult::TransferInProgress { transfer } => {
                public::RequestResult::TransferInProgress { transfer }
            }
            RequestResult::SchemaNotReady(reason) => public::RequestResult::SchemaNotReady(reason),
            RequestResult::PendingUnreachable => public::RequestResult::PendingUnreachable,
            RequestResult::ProjectCreationPending { project } => {
                public::RequestResult::ProjectCreationPending { project }
            }
            RequestResult::Overloaded => public::RequestResult::Overloaded,
            RequestResult::ScopeUnavailable(reason) => {
                public::RequestResult::ScopeUnavailable(reason)
            }
        };
        Ok(public::WriteReply {
            result,
            reader: reply.reader,
        })
    }

    async fn public_decision(
        &self,
        scope: &RequestEffectsScope,
        request: &RequestId,
        decision: RequestDecision,
    ) -> Result<public::RequestDecision, BoardError> {
        Ok(match decision {
            RequestDecision::Accepted { effects, change } => {
                let next = if let Some(cursor) = effects.next() {
                    Some(
                        encode_board_cursor(
                            self.receipt_signing_key().await?,
                            &ReceiptContinuation {
                                scope: scope.clone(),
                                request: request.clone(),
                                count: effects.total_count(),
                                digest: effects.digest().clone(),
                                cursor: cursor.clone(),
                            },
                        )
                        .map_err(|_| BoardError::board_unavailable())?,
                    )
                } else {
                    None
                };
                public::RequestDecision::Accepted {
                    effects: public::DecisionEffectsPage::from_native(effects, next)
                        .map_err(|_| BoardError::board_unavailable())?,
                    change,
                }
            }
            RequestDecision::Rejected { reasons } => public::RequestDecision::Rejected { reasons },
        })
    }
}

fn invalid_cursor() -> public::RequestEffectsRead {
    public::RequestEffectsRead::Refused {
        reason: RequestEffectsRefusal::CursorInvalid,
    }
}
