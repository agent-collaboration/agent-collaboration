//! Typed thread board calls; uncertain writes are never retransmitted.
use super::subscription_resource;
use super::{BoardClientError, CollaborationClient};
use crate::ClientError;
use collaboration_protocol::{
    ThreadSubscribeRequest, ThreadSubscriptionWaitRequest, ThreadSubscriptionWaitResult,
    ThreadSubscriptionsRequest, ThreadSubscriptionsResult, ThreadUnsubscribeRequest,
};
use message_board::public_replies::WriteReply;
use message_board::*;
use std::time::Duration;
impl CollaborationClient {
    pub async fn thread_renew(
        &self,
        request: ThreadRenewRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = subscription_resource(&request.scope);
        self.board_mutation_call("thread_renew", request, resource)
            .await
    }
    pub async fn thread_post(
        &self,
        request: ThreadPostRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Post {
            post_id: request.post_id.clone(),
        };
        self.board_mutation_call("thread_post", request, resource)
            .await
    }
    pub async fn thread_show(
        &self,
        request: ThreadShowRequest,
    ) -> Result<ThreadShowResult, BoardClientError> {
        self.board_call("thread_show", request).await
    }
    pub async fn thread_refile(
        &self,
        request: ThreadRefileRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Thread {
            thread_id: request.thread.thread.clone(),
        };
        self.board_mutation_call("thread_refile", request, resource)
            .await
    }
    pub async fn thread_resolve(
        &self,
        request: ThreadResolveRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Thread {
            thread_id: request.thread.thread.clone(),
        };
        self.board_mutation_call("thread_resolve", request, resource)
            .await
    }
    pub async fn thread_unresolve(
        &self,
        request: ThreadUnresolveRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Thread {
            thread_id: request.thread.thread.clone(),
        };
        self.board_mutation_call("thread_unresolve", request, resource)
            .await
    }
    pub async fn thread_watch(
        &self,
        request: ThreadWatchRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Thread {
            thread_id: request.thread.thread.clone(),
        };
        self.board_mutation_call("thread_watch", request, resource)
            .await
    }
    pub async fn thread_unwatch(
        &self,
        request: ThreadUnwatchRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Thread {
            thread_id: request.thread.thread.clone(),
        };
        self.board_mutation_call("thread_unwatch", request, resource)
            .await
    }
    pub async fn thread_list(
        &self,
        request: ThreadListRequest,
    ) -> Result<ThreadListResult, BoardClientError> {
        self.board_call("thread_list", request).await
    }
    pub async fn thread_create(
        &self,
        request: ThreadCreateRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Post {
            post_id: request.post_id.clone(),
        };
        self.board_mutation_call("thread_create", request, resource)
            .await
    }
    pub async fn thread_join(
        &self,
        request: ThreadJoinRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Thread {
            thread_id: request.thread.thread.clone(),
        };
        self.board_mutation_call("thread_join", request, resource)
            .await
    }
    pub async fn thread_leave(
        &self,
        request: ThreadLeaveRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Thread {
            thread_id: request.thread.thread.clone(),
        };
        self.board_mutation_call("thread_leave", request, resource)
            .await
    }
    pub async fn thread_participant_list(
        &self,
        request: ThreadParticipantListRequest,
    ) -> Result<ThreadParticipantListResult, BoardClientError> {
        self.board_call("thread_participant_list", request).await
    }
    pub async fn thread_subscribe(
        &self,
        request: ThreadSubscribeRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = subscription_resource(&request.scope);
        self.board_mutation_call("thread_subscribe", request, resource)
            .await
    }
    pub async fn thread_unsubscribe(
        &self,
        request: ThreadUnsubscribeRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = subscription_resource(&request.scope);
        self.board_mutation_call("thread_unsubscribe", request, resource)
            .await
    }
    pub async fn thread_subscriptions(
        &self,
        request: ThreadSubscriptionsRequest,
    ) -> Result<ThreadSubscriptionsResult, BoardClientError> {
        self.board_call("thread_subscriptions", request).await
    }
    pub async fn thread_wait(
        &self,
        request: ThreadSubscriptionWaitRequest,
        timeout: Duration,
    ) -> Result<ThreadSubscriptionWaitResult, BoardClientError> {
        request
            .validate()
            .map_err(|_| ClientError::InvalidRequest("invalid Thread Wait request"))?;
        let actor = Box::new(request.actor.clone());
        let filter = request.filter.clone();
        let renewals = request.renewals.clone();
        let params = serde_json::to_value(&request)
            .map_err(|_| ClientError::Protocol("invalid Thread Wait request"))?;
        let result = match self
            .connection
            .call_with_timeout("thread_wait", params, timeout)
            .await
        {
            Ok(result) => result,
            Err(ClientError::Rejected {
                code: -32050,
                data: Some(data),
            }) => {
                return Err(BoardClientError::Rejected(Box::new(
                    serde_json::from_value(data)
                        .map_err(|_| ClientError::Protocol("invalid board failure"))?,
                )));
            }
            Err(ClientError::Overloaded { message }) => {
                return Err(BoardClientError::Rejected(Box::new(
                    crate::admission_overload::board(message),
                )));
            }
            Err(ClientError::Timeout)
            | Err(ClientError::Transport(_))
            | Err(ClientError::Protocol(_)) => {
                return Err(BoardClientError::WaitOutcomeUnknown {
                    actor,
                    filter,
                    renewals,
                });
            }
            Err(error) => return Err(error.into()),
        };
        match serde_json::from_value(result) {
            Ok(result) => Ok(result),
            Err(_) => Err(BoardClientError::WaitOutcomeUnknown {
                actor,
                filter,
                renewals,
            }),
        }
    }
}
