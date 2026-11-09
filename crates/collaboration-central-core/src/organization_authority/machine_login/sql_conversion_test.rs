//! B3's switch on the real Tokio store: a Fleet built by today's 0001/0002
//! code, then the staged v3/v4 through the migration framework. Fixture
//! initialization only; production bootstrap is unchanged.
use super::super::sql_store_fixture::{LegacyMachine, SqlStoreFixture, TestError};
use super::super::test_fixtures::uuid7;
use crate::machine_enrollment_record::{READMISSION_TASK_KIND, REVOKE_TASK_KIND};
use durable_steps::{
    AtomicStore, JournalScope, OperationContext, OperationFailure, RequestKey, Task,
};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

const V4_SERVICE: &str = "0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7d";

fn machine(peer: u64, service: &str, last_seen: Option<&str>, created_at: &str) -> LegacyMachine {
    LegacyMachine {
        peer: uuid7(peer),
        service: service.to_owned(),
        label: format!("machine {peer}"),
        last_seen: last_seen.map(str::to_owned),
        created_at: created_at.to_owned(),
    }
}

async fn history(fixture: &SqlStoreFixture) -> Result<Vec<String>, TestError> {
    fixture
        .select(
            "SELECT json_array(version, hex(checksum)) FROM central_schema_migrations \
             WHERE corpus = 'central-registry' ORDER BY version"
                .to_owned(),
        )
        .await
}

async fn installations(fixture: &SqlStoreFixture) -> Result<Vec<String>, TestError> {
    fixture
        .select(
            "SELECT json_array(id, former_service_id, label, enrollment_generation, last_seen_at, \
             created_at) FROM machine_installations ORDER BY id"
                .to_owned(),
        )
        .await
}

async fn legacy_rows(fixture: &SqlStoreFixture) -> Result<Vec<String>, TestError> {
    fixture
        .select(
            "SELECT json_array(machine_id, service_id, machine_label, access_client_id, \
             credential_hash, state, last_seen_at, created_at) FROM machine_enrollments \
             ORDER BY machine_id"
                .to_owned(),
        )
        .await
}

#[tokio::test]
async fn todays_machines_convert_once_keeping_ids_labels_former_ids_and_exact_times()
-> Result<(), TestError> {
    let fixture = SqlStoreFixture::pre_b3(vec![
        machine(0x10, V4_SERVICE, None, "2026-11-08T12:00:00.123Z"),
        machine(
            0x11,
            &uuid7(0x911),
            Some("2026-11-08T13:30:00.5+01:00"),
            "2026-11-08T12:00:00Z",
        ),
    ])
    .await?;
    let old_history = history(&fixture).await?;

    assert_eq!(fixture.initialize_b3().await?, 2);

    assert_eq!(
        installations(&fixture).await?,
        vec![
            format!(
                r#"["{}","{V4_SERVICE}","machine 16",0,null,1794139200123]"#,
                uuid7(0x10)
            ),
            format!(
                r#"["{}","{}","machine 17",0,1794141000500,1794139200000]"#,
                uuid7(0x11),
                uuid7(0x911)
            ),
        ]
    );
    // The new enrollments table starts empty and has none of the Access columns.
    assert!(
        fixture
            .select("SELECT id FROM machine_enrollments".to_owned())
            .await?
            .is_empty()
    );
    assert!(
        fixture
            .select("SELECT access_client_id FROM machine_enrollments".to_owned())
            .await
            .is_err()
    );
    // Versions 1 and 2 keep their checksums; 3 and 4 are recorded.
    let new_history = history(&fixture).await?;
    assert_eq!(new_history[..2], old_history[..]);
    assert_eq!(new_history.len(), 4);

    // A second initialization applies nothing and changes nothing.
    let converted = installations(&fixture).await?;
    assert_eq!(fixture.initialize_b3().await?, 0);
    assert_eq!(installations(&fixture).await?, converted);
    assert_eq!(history(&fixture).await?, new_history);
    Ok(())
}

#[tokio::test]
async fn an_empty_fleet_converts_and_reinitializes() -> Result<(), TestError> {
    let fixture = SqlStoreFixture::pre_b3(Vec::new()).await?;
    assert_eq!(fixture.initialize_b3().await?, 2);
    assert!(installations(&fixture).await?.is_empty());
    assert_eq!(fixture.initialize_b3().await?, 0);
    Ok(())
}

/// A failed conversion leaves today's schema, rows and history as they were.
async fn assert_conversion_refused(
    fixture: &SqlStoreFixture,
    version: u64,
    cause: &str,
) -> Result<(), TestError> {
    let rows = legacy_rows(fixture).await?;
    let old_history = history(fixture).await?;

    let refused = fixture
        .initialize_b3()
        .await
        .expect_err("the switch must refuse");
    let failure = crate::migration_failure::MigrationFailure::from_rollback(&refused)
        .expect("a migration rollback names its corpus, version and cause");
    assert_eq!(
        (failure.corpus.as_str(), failure.version),
        ("central-registry", version)
    );
    assert!(failure.reason.contains(cause), "{}", failure.reason);

    assert_eq!(legacy_rows(fixture).await?, rows);
    assert_eq!(history(fixture).await?, old_history);
    assert!(
        fixture
            .select("SELECT id FROM machine_installations".to_owned())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn an_invalid_or_finer_than_millisecond_time_fails_the_switch_and_changes_nothing()
-> Result<(), TestError> {
    for created_at in ["yesterday", "2026-11-08T12:00:00.1234Z"] {
        let fixture = SqlStoreFixture::pre_b3(vec![
            machine(0x10, V4_SERVICE, None, "2026-11-08T12:00:00Z"),
            machine(0x11, &uuid7(0x911), None, created_at),
        ])
        .await?;
        assert_conversion_refused(&fixture, 3, "machine_enrollments.created_at").await?;
    }
    Ok(())
}

/// A stand-in for today's journal task kinds: admitted, never run.
struct LegacyTask<const REVOKE: bool, Store>(PhantomData<fn() -> Store>);
#[derive(Serialize, Deserialize)]
struct NoInput;
impl<const REVOKE: bool, Store: AtomicStore> Task for LegacyTask<REVOKE, Store> {
    const KIND: &'static str = if REVOKE {
        REVOKE_TASK_KIND
    } else {
        READMISSION_TASK_KIND
    };
    const VERSION: u32 = 2;
    type Store = Store;
    type Input = NoInput;
    type Output = ();
    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        _: &mut Context,
        _: &NoInput,
    ) -> Result<(), OperationFailure> {
        Ok(())
    }
}

#[tokio::test]
async fn an_unfinished_legacy_revocation_or_readmission_refuses_the_switch() -> Result<(), TestError>
{
    for revoke in [true, false] {
        let fixture = SqlStoreFixture::pre_b3(vec![machine(
            0x10,
            V4_SERVICE,
            None,
            "2026-11-08T12:00:00Z",
        )])
        .await?;
        fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let key = RequestKey::new(format!("legacy:{revoke}"))
                        .map_err(durable_steps::StoreError::backend)?;
                    let admitted = if revoke {
                        scope
                            .admit::<LegacyTask<true, super::super::sql_store_fixture::FixtureObject>>(
                                &key, &NoInput,
                            )
                            .await
                    } else {
                        scope
                            .admit::<LegacyTask<false, super::super::sql_store_fixture::FixtureObject>>(
                                &key, &NoInput,
                            )
                            .await
                    };
                    admitted.map_err(|refusal| durable_steps::StoreError::Aborted {
                        reason: format!("{refusal:?}"),
                    })?;
                    Ok(())
                })
            })
            .await?;

        assert_conversion_refused(&fixture, 3, "is unfinished").await?;
    }
    Ok(())
}

/// A failure after v3's DROP and copy (here v4, whose table already exists)
/// rolls the whole switch back: today's table and rows return untouched.
#[tokio::test]
async fn a_failure_after_the_destructive_step_rolls_the_whole_switch_back() -> Result<(), TestError>
{
    let fixture = SqlStoreFixture::pre_b3(vec![
        machine(0x10, V4_SERVICE, None, "2026-11-08T12:00:00Z"),
        machine(0x11, &uuid7(0x911), None, "2026-11-08T12:00:01Z"),
    ])
    .await?;
    fixture
        .execute("CREATE TABLE machine_allowances (id TEXT)".to_owned())
        .await?;

    assert_conversion_refused(&fixture, 4, "machine_allowances").await
}
