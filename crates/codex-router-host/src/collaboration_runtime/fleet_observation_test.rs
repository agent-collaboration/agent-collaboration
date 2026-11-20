use super::*;

#[tokio::test]
async fn missing_mail_owner_reports_sent_mail_age_unavailable() {
    let observed = observe_fleet_sources(&Weak::new()).await;
    assert!(matches!(
        observed,
        crate::FleetHostObservation::Available {
            stuck_mail: StuckMailSummary::Unavailable { .. },
        }
    ));
}
