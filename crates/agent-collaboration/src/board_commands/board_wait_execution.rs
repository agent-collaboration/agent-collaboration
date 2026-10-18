//! CLI-generated renewal keys belong to the caller and travel with the wait result.
use super::board_preparation::PendingSubscriptionWait;
use collaboration_client::{BoardClientError, CollaborationClient, board::*, protocol::*};
use std::collections::BTreeMap;

pub(super) async fn execute(
    client: &CollaborationClient,
    mut pending: PendingSubscriptionWait,
) -> Result<ThreadSubscriptionWaitResult, BoardClientError> {
    if pending.request.renewals.is_empty() {
        pending.request.renewals = renewals_for_filter(client, &pending.request).await?;
    }
    client.thread_wait(pending.request, pending.timeout).await
}
async fn renewals_for_filter(
    client: &CollaborationClient,
    request: &ThreadSubscriptionWaitRequest,
) -> Result<Vec<SubscriptionRenewalEntry>, BoardClientError> {
    let subscriptions = client
        .thread_subscriptions(ThreadSubscriptionsRequest {
            actor: request.actor.clone(),
        })
        .await?
        .subscriptions;
    let mut root_projects = BTreeMap::new();
    if let ThreadSubscriptionWaitFilter::Roots { threads } = &request.filter {
        for thread in threads {
            root_projects.insert(thread.thread.clone(), thread_project(client, thread).await?);
        }
    }
    let mut renewals = Vec::new();
    for subscription in &subscriptions {
        if subscription.policy.mode() != SubscriptionMode::Poll
            || subscription.state != ThreadSubscriptionState::Active
        {
            continue;
        }
        let covered = match (&request.filter, &subscription.scope) {
            (ThreadSubscriptionWaitFilter::All {}, _) => true,
            (
                ThreadSubscriptionWaitFilter::Project { project_id },
                SubscriptionScope::Project {
                    project_id: scope_project,
                },
            ) => project_id == scope_project,
            (
                ThreadSubscriptionWaitFilter::Project { project_id },
                SubscriptionScope::Thread { .. },
            ) => subscription.project_id == *project_id,
            (
                ThreadSubscriptionWaitFilter::Roots { threads },
                SubscriptionScope::Thread { thread_id },
            ) => threads.iter().any(|thread| {
                thread.thread == *thread_id
                    && root_projects.get(thread_id) == Some(&subscription.project_id)
            }),
            (
                ThreadSubscriptionWaitFilter::Roots { threads },
                SubscriptionScope::Project { project_id },
            ) => threads.iter().any(|thread| {
                let thread_id = &thread.thread;
                root_projects.get(thread_id) == Some(project_id)
                    && !subscriptions
                        .iter()
                        .any(|row| row.scope == SubscriptionScope::thread(thread_id.clone()))
            }),
        };
        if covered {
            renewals.push(SubscriptionRenewalEntry {
                request_id: RequestId::generate(),
                resubmits: None,
                observed_epoch: None,
                scope: match &subscription.scope {
                    SubscriptionScope::Project { project_id } => SubscriptionTarget::Project {
                        project_id: project_id.clone(),
                    },
                    SubscriptionScope::Thread { thread_id } => SubscriptionTarget::Thread {
                        thread: ThreadRef {
                            thread: thread_id.clone(),
                            project: Some(subscription.project_id.clone()),
                        },
                    },
                },
            });
        }
    }
    Ok(renewals)
}
async fn thread_project(
    client: &CollaborationClient,
    thread: &ThreadRef,
) -> Result<ProjectId, BoardClientError> {
    if let Some(project) = &thread.project {
        return Ok(project.clone());
    }
    client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: thread.clone(),
            reader: ReadActor::Anonymous {},
        })
        .await
        .map(|result| result.project_id)
}
