//! Actual committed Central imports survive a suppressed successful answer.
use crate::{
    answer_loss_edge::{AnswerLossEdge, LostAnswer},
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::native_fixture,
    publication::preparation,
};
use board_central_test_support::TestCentral;
use board_staging_cutover::{
    PreparedProjectBundle, PublicationError, PublicationStage, StagingPublisher,
};
use codex_router_secret_store::central_credentials::CentralCredential;
use collaboration_central_core::{
    AdminImportPhase, AdminImportRecord, AdminProjectImportAnswer, AdminProjectImportRequest,
};
use message_board::ActorOrigin;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn boxed(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}

pub(super) fn publisher(endpoint: &str, area: &Path) -> TestResult<StagingPublisher> {
    let path = area.join(format!("credential-{}.json", uuid::Uuid::now_v7()));
    CentralCredential::Owner {
        endpoint: endpoint.to_owned(),
        assertion: collaboration_central_conformance::owner_assertion()?,
        expires_at: "2099-01-01T00:00:00Z".to_owned(),
    }
    .write_private_file(&path)?;
    Ok(StagingPublisher::from_owner_file(
        url::Url::parse(endpoint)?,
        &path,
    )?)
}

async fn status(
    central: &TestCentral,
    bundle: &PreparedProjectBundle,
) -> TestResult<AdminImportRecord> {
    let answer: AdminProjectImportAnswer = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .post(format!(
            "{}/admin/v1/import/projects/{}",
            central.base_url(),
            bundle.manifest().source.selected.project.as_str()
        ))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&AdminProjectImportRequest::Status {
            digest: bundle.manifest().header.digest.clone(),
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let AdminProjectImportAnswer::Progress(record) = answer else {
        return Err("actual import continuation is absent".into());
    };
    Ok(record)
}

async fn suppressed_answer_replay(selected: LostAnswer) -> TestResult {
    // Arrange a real source, enrolled Central, and one causal post-commit HTTP cut.
    let fixture = CutoverHostFixture::new(None).map_err(boxed)?;
    let source = native_fixture(&fixture).await.map_err(boxed)?;
    let origin = ActorOrigin {
        installation: String::from(fixture.service.clone()).try_into()?,
        machine: "answer loss source".to_owned().try_into()?,
    };
    let central_root = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(central_root.path(), &origin, fixture.peer).await?;
    let result = async {
        let edge = AnswerLossEdge::start(central.base_url(), selected).await?;
        let result = async {
            let area = tempfile::tempdir()?;
            std::fs::set_permissions(area.path(), std::fs::Permissions::from_mode(0o700))?;
            let input = preparation(&origin, &source, &edge.endpoint)?;
            let mut offline = fixture.acquire().map_err(boxed)?;
            let bundle =
                PreparedProjectBundle::prepare(&mut offline, source.selected, input, area.path())
                    .await?;
            let directory: PathBuf = bundle.directory().to_owned();
            let owner = publisher(&edge.endpoint, area.path())?;
            // Act: the edge forwards exact bytes, observes success, then withholds that answer.
            let first = owner.publish(&mut offline, &bundle).await;
            let suppressed = edge.suppressed()?;
            let before = status(&central, &bundle).await?;
            assert_eq!(before.header, bundle.manifest().header);
            match (selected, suppressed) {
                (LostAnswer::Begin, AdminProjectImportAnswer::Ready { import_id, .. })
                | (LostAnswer::Page, AdminProjectImportAnswer::Staged { import_id, page: 0 }) => {
                    assert_eq!(before.import_id, import_id)
                }
                (LostAnswer::Complete, AdminProjectImportAnswer::Imported { head }) => {
                    assert_eq!(head, bundle.manifest().header.manifest.position)
                }
                _ => return Err("cut did not observe the selected successful response".into()),
            }
            // An implementation may reconcile immediately, but it may never claim an uncommitted import.
            match first {
                Ok(confirmed) => {
                    assert_eq!(
                        before.phase,
                        AdminImportPhase::Complete {
                            position: bundle.manifest().header.manifest.position
                        }
                    );
                    assert!(confirmed.matches_bundle(&bundle)?);
                }
                Err(PublicationError::OutcomeUnknown { stage, .. }) => {
                    assert!(matches!(
                        (selected, stage),
                        (LostAnswer::Begin, PublicationStage::Begin)
                            | (LostAnswer::Page, PublicationStage::Page { index: 0 })
                            | (LostAnswer::Complete, PublicationStage::Complete)
                    ));
                    assert!(
                        bundle
                            .manifest()
                            .source_fingerprint
                            .matches(bundle.manifest().source_directory.as_path())?
                    );
                    assert!(!bundle.manifest().backup_directory.exists());
                }
                Err(error) => return Err(error.into()),
            }
            // Reopen with fresh in-memory owner access; the immutable run and request IDs remain.
            drop(owner);
            drop(bundle);
            drop(offline);
            let reopened = PreparedProjectBundle::open(&directory)?;
            let mut offline = fixture.acquire().map_err(boxed)?;
            let confirmed = publisher(&edge.endpoint, area.path())?
                .publish(&mut offline, &reopened)
                .await?;
            let after = status(&central, &reopened).await?;
            assert_eq!(after.import_id, before.import_id);
            assert_eq!(confirmed.import_id(), before.import_id);
            assert_eq!(after.header, before.header);
            assert_eq!(
                after.phase,
                AdminImportPhase::Complete {
                    position: reopened.manifest().header.manifest.position
                }
            );
            assert!(confirmed.matches_bundle(&reopened)?);
            assert!(!reopened.manifest().backup_directory.exists());
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let stopped = edge.shutdown().await;
        result?;
        stopped?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = central.shutdown().await;
    result?;
    stopped?;
    Ok(())
}

#[tokio::test]
async fn a_committed_begin_answer_loss_reuses_the_same_import_on_reopen() -> TestResult {
    suppressed_answer_replay(LostAnswer::Begin).await
}
#[tokio::test]
async fn a_committed_page_answer_loss_reuses_the_same_import_on_reopen() -> TestResult {
    suppressed_answer_replay(LostAnswer::Page).await
}
#[tokio::test]
async fn a_committed_complete_answer_loss_reuses_the_same_import_on_reopen() -> TestResult {
    suppressed_answer_replay(LostAnswer::Complete).await
}
