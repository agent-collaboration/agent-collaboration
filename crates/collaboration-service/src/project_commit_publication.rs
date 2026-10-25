//! Admission callers prompt the same publication owners after their decision.
use crate::{NotificationPublisher, SubscriptionDeliveryService};
use message_board::{Identity, ProjectId};

/// Post-commit failures are logged; they never replace the recorded decision.
pub async fn after_project_commit_publish_and_reconcile(
    publisher: Option<&NotificationPublisher>,
    delivery: Option<&SubscriptionDeliveryService>,
    actor: Identity,
    project: ProjectId,
) {
    if let Some(publisher) = publisher
        && let Err(error) = publisher.queue_project(project.clone()).await
    {
        tracing::warn!(%error, "notification publisher could not be queued after admission");
    }
    if let Some(delivery) = delivery
        && let Err(error) = delivery.reconcile_project(actor, project).await
    {
        tracing::warn!(%error, "board delivery reconciliation failed after admission");
    }
}
