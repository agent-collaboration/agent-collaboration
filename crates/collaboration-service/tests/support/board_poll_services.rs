//! The shared native mail, publication and reader composition for board fixtures.
use super::{TestError, fixture_machine_identity, fixture_peer, fixture_subscription_service};
use collaboration_service::{ApplicationFixture, ProjectAdmission};
use std::{path::Path, sync::Arc};

pub struct FixturePollServices {
    pub identity: ApplicationFixture,
    pub delivery: collaboration_service::SubscriptionDeliveryService,
    pub native: super::native_mail_owner::NativeMailOwner,
    pub publisher: Arc<collaboration_service::NotificationPublisher>,
}

/// Compose the real owners before a request can create its reader task.
pub async fn fixture_poll_services(
    directory: &Path,
    admission: &Arc<ProjectAdmission>,
    identity: ApplicationFixture,
) -> Result<FixturePollServices, TestError> {
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(directory.join("interaction.sqlite")).await?,
    );
    let automation = Arc::new(tokio::sync::Mutex::new(
        automation_storage::AutomationStore::open(&directory.join("automation.sqlite")).await?,
    ));
    let routes = Arc::new(collaboration_runtime::SessionDeliveryRouter::new(Vec::new()));
    let delivery = fixture_subscription_service(admission, &interactions, &routes)?;
    let identity = identity
        .with_automation_store(automation)
        .with_subscription_delivery_service(delivery.clone(), routes.clone());
    let (identity, native) = super::native_mail_owner::NativeMailOwner::start(
        identity,
        fixture_peer()?,
        collaboration_service::MailPushDeliveryProps {
            interactions: interactions.clone(),
            actors: delivery.clone(),
            presence: routes,
            machine: fixture_machine_identity()?,
            clock: Arc::new(collaboration_service::SystemSubscriptionClock),
        },
    )?;
    let publisher = Arc::new(collaboration_service::NotificationPublisher::new(
        admission.clone(),
        interactions,
        Arc::downgrade(&native.runtime_handle()),
    ));
    let identity = identity.with_notification_publisher(publisher.clone());
    delivery.start().await?;
    Ok(FixturePollServices {
        identity,
        delivery,
        native,
        publisher,
    })
}
