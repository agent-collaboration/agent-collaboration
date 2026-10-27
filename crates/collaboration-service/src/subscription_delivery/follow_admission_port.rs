//! Weak paired ports keep the reader's windows separate from allocation and publication.
use super::SubscriptionDeliveryService;
use crate::{NotificationPublisher, ProjectAdmission};
use std::sync::{Arc, RwLock, Weak};

#[derive(Clone)]
pub(in crate::service_identity::subscription_delivery) struct FollowAdmissionPort {
    pub(in crate::service_identity::subscription_delivery) admission: Weak<ProjectAdmission>,
    pub(in crate::service_identity::subscription_delivery) publisher: Weak<NotificationPublisher>,
}
pub(in crate::service_identity::subscription_delivery) type FollowAdmissionSlot =
    Arc<RwLock<Option<FollowAdmissionPort>>>;

impl SubscriptionDeliveryService {
    pub(crate) async fn prompt_follow_coverage(&self, projects: &[message_board::ProjectId]) {
        let port = self
            .inner
            .follow_admission
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(publisher) = port.and_then(|port| port.publisher.upgrade())
            && let Err(error) = publisher.prompt_follow_coverage(projects).await
        {
            tracing::warn!(%error, "committed mail status coverage hint delayed");
        }
    }

    pub(crate) fn attach_follow_admission(
        &self,
        admission: Weak<ProjectAdmission>,
        publisher: Weak<NotificationPublisher>,
    ) {
        *self
            .inner
            .follow_admission
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(FollowAdmissionPort {
            admission,
            publisher,
        });
    }
}
