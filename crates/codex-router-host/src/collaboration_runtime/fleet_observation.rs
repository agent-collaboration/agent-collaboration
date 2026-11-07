//! Link state and mail-age reads have independent failure domains.
use board_fleet_sync::FleetLinkObservation;
use collaboration_protocol::StuckMailSummary;
use interaction_storage::InteractionStore;
use std::{sync::Weak, time::Duration};

const OBSERVATION_READ_BOUND: Duration = Duration::from_secs(5);

pub(super) async fn observe_fleet_sources(
    link: &FleetLinkObservation,
    interactions: &Weak<InteractionStore>,
) -> crate::FleetHostObservation {
    let link_read = tokio::time::timeout(OBSERVATION_READ_BOUND, link.read());
    let mail_read = async {
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
    };
    let (central, stuck_mail) = tokio::join!(link_read, mail_read);
    match central {
        Ok(Ok(central)) => crate::FleetHostObservation::Available {
            central,
            stuck_mail,
        },
        Ok(Err(error)) => crate::FleetHostObservation::Unavailable {
            message: error.to_string(),
        },
        Err(_) => crate::FleetHostObservation::Unavailable {
            message: "central link read timed out".to_owned(),
        },
    }
}

#[cfg(test)]
#[path = "fleet_observation_test.rs"]
mod tests;
