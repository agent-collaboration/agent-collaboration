use super::{PausePoint, PausePoints};
use workspace_ids::WorkspaceId;

fn workspace() -> WorkspaceId {
    WorkspaceId::try_from("019f0000-0000-7000-8000-000000000001".to_owned()).unwrap()
}

#[tokio::test]
async fn an_unarmed_point_passes_straight_through() {
    let pauses = PausePoints::default();
    pauses.reached(&workspace(), PausePoint::AfterReads).await;
    assert!(pauses.held(&workspace()).is_empty());
}

#[tokio::test]
async fn an_armed_point_holds_one_request_until_released() {
    let pauses = PausePoints::default();
    let mut hold = pauses.hold(&workspace(), PausePoint::AfterStoreApplied);
    let request_pauses = pauses.clone();
    let request = tokio::spawn(async move {
        request_pauses
            .reached(&workspace(), PausePoint::AfterReads)
            .await;
        request_pauses
            .reached(&workspace(), PausePoint::AfterStoreApplied)
            .await;
    });
    hold.until_held().await;
    assert_eq!(
        pauses.held(&workspace()),
        vec![PausePoint::AfterStoreApplied]
    );
    assert!(!request.is_finished());
    hold.release();
    request.await.unwrap();
    assert!(pauses.held(&workspace()).is_empty());
    // The arm is consumed: a second request passes.
    pauses
        .reached(&workspace(), PausePoint::AfterStoreApplied)
        .await;
}

#[tokio::test]
async fn dropping_a_held_request_unlists_it() {
    let pauses = PausePoints::default();
    let mut hold = pauses.hold(&workspace(), PausePoint::AfterNoticesCommitted);
    let request_pauses = pauses.clone();
    let request = tokio::spawn(async move {
        request_pauses
            .reached(&workspace(), PausePoint::AfterNoticesCommitted)
            .await;
    });
    hold.until_held().await;
    request.abort();
    let _ = request.await;
    assert!(pauses.held(&workspace()).is_empty());
}
