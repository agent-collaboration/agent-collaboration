//! Read sent-mail age from the interaction owner.
use collaboration_protocol::StuckMailSummary;
use interaction_storage::InteractionStore;
use std::{sync::Weak, time::Duration};

pub(crate) type FleetObservationFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = crate::FleetHostObservation> + Send + 'static>,
>;

const OBSERVATION_READ_BOUND: Duration = Duration::from_secs(5);

pub(super) async fn observe_fleet_sources(
    interactions: &Weak<InteractionStore>,
) -> crate::FleetHostObservation {
    let stuck_mail = async {
        let Some(interactions) = interactions.upgrade() else {
            return StuckMailSummary::Unavailable {
                message: "interaction owner is unavailable".to_owned(),
            };
        };
        match tokio::time::timeout(
            OBSERVATION_READ_BOUND,
            interactions.stuck_sent_mail(chrono::Utc::now()),
        )
        .await
        {
            Ok(Ok(summary)) => summary,
            Ok(Err(error)) => StuckMailSummary::Unavailable {
                message: error.to_string(),
            },
            Err(_) => StuckMailSummary::Unavailable {
                message: "sent-mail age read timed out".to_owned(),
            },
        }
    }
    .await;
    crate::FleetHostObservation::Available { stuck_mail }
}

#[cfg(test)]
#[path = "fleet_observation_test.rs"]
mod tests;
