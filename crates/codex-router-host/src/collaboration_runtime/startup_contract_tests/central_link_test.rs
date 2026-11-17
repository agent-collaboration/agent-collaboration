//! The actual Host composition starts the central link for its installation.
use super::*;
use collaboration_runtime::{CentralLinkRefusal, CentralLinkStatus};

#[tokio::test]
async fn the_host_starts_its_central_link_and_reports_not_enrolled_before_b3() {
    // Arrange: a whole Host with no sign-in token source (B3 not landed).
    let fixture = start_host(false).await;

    // Act
    let link = fixture
        .runtime
        .central_link()
        .expect("the provider-operations store opened, so the link started");
    let status = link.link_status().await;
    fixture.runtime.shutdown().await.expect("runtime cleanup");
    let after_shutdown = link.link_status().await;

    // Assert: not enrolled; new code needed, and the driver stopped with the Host.
    assert_eq!(
        status,
        Some(CentralLinkStatus::Refused {
            reason: CentralLinkRefusal::CredentialRefused
        })
    );
    assert_eq!(after_shutdown, None, "the link driver stopped at shutdown");
}
