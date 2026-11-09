//! Every SQL case on the Tokio host's real store (sqlx-turso), one test each,
//! each on a fresh store. workerd runs the same cases on Durable Object SQLite
//! (`machine_login_cases_workerd`).
use super::{CasePhase, SQL_CASES, run_sql_case};
use crate::CoreHost;
use crate::fleet_storage_fixture::FixtureIo;
use fleet_sync_protocol::ReceiverId;
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore, TokioObject};
use std::sync::Arc;

type FixtureObject = TokioObject<CoreHost<FixtureIo>>;
type TestError = Box<dyn std::error::Error + Send + Sync>;

async fn open(
    root: &std::path::Path,
) -> Result<(ObjectStore<CoreHost<FixtureIo>>, Arc<FixtureObject>), TestError> {
    let objects = ObjectStore::open(root).await?;
    let host = Arc::new(CoreHost::for_store(
        FixtureIo,
        ReceiverId::new(durable_steps::Uuid::now_v7())?,
    ));
    let object = objects
        .object(ObjectKey::new("fleet")?, Default::default(), host)
        .await?;
    Ok((objects, object))
}

async fn run(name: &str) -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let (objects, object) = open(root.path()).await?;
    let result =
        run_sql_case::<FixtureObject, sqlx_turso::Turso>(name, CasePhase::Run, object).await;
    objects.close().await?;
    Ok(result?)
}

macro_rules! sql_cases {
    ($($test:ident => $name:literal),+ $(,)?) => {
        $(
            #[tokio::test]
            async fn $test() -> Result<(), TestError> {
                run($name).await
            }
        )+
        const NATIVE_CASES: &[&str] = &[$($name),+];
    };
}

sql_cases! {
    staged_corpus_initializes_once => "stagedCorpusInitializesOnce",
    approval_and_first_grant => "approvalAndFirstGrant",
    repeat_pending_owner => "repeatPendingOwner",
    refusals_write_nothing => "refusalsWriteNothing",
    records_keep_their_own_keys => "recordsKeepTheirOwnKeys",
    first_grant_wins => "firstGrantWins",
    code_deadline_lapses => "codeDeadlineLapses",
    failure_after_writes_rolls_back => "failureAfterWritesRollsBack",
    admission_reads_live => "admissionReadsLive",
    revocation_ends_machine_and_earlier_approvals => "revocationEndsMachineAndEarlierApprovals",
    converted_former_id_links_own_machine_only => "convertedFormerIdLinksOwnMachineOnly",
    engine_refuses_second_enrolled => "engineRefusesSecondEnrolled",
    engine_refuses_duplicate_former_id => "engineRefusesDuplicateFormerId",
    engine_refuses_duplicate_allowance => "engineRefusesDuplicateAllowance",
    engine_refuses_duplicate_request_or_client => "engineRefusesDuplicateRequestOrClient",
    invalid_stored_rows_refused_on_read => "invalidStoredRowsRefusedOnRead",
    former_id_never_replaced => "formerIdNeverReplaced",
    todays_machines_convert_once => "todaysMachinesConvertOnce",
    empty_fleet_converts => "emptyFleetConverts",
    refusal_names_every_refused_row => "refusalNamesEveryRefusedRow",
    unfinished_legacy_revocation_refuses => "unfinishedLegacyRevocationRefuses",
    unfinished_legacy_readmission_refuses => "unfinishedLegacyReadmissionRefuses",
    failure_after_destructive_step_rolls_back => "failureAfterDestructiveStepRollsBack",
}

/// Written, then read back by a new owning store over the same persistence,
/// in this process.
#[tokio::test]
async fn persistence() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let (objects, object) = open(root.path()).await?;
    run_sql_case::<FixtureObject, sqlx_turso::Turso>("persistence", CasePhase::Write, object)
        .await?;
    objects.close().await?;

    let (objects, object) = open(root.path()).await?;
    let read =
        run_sql_case::<FixtureObject, sqlx_turso::Turso>("persistence", CasePhase::Read, object)
            .await;
    objects.close().await?;
    Ok(read?)
}

/// Every case has its native test, and no test names a case that isn't one.
#[test]
fn every_case_runs_on_tokio() {
    let mut native = NATIVE_CASES.to_vec();
    native.push("persistence");
    native.sort_unstable();
    let mut cases = SQL_CASES.to_vec();
    cases.sort_unstable();
    assert_eq!(native, cases);
}
