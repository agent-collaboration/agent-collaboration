//! Repository attachment identity and original provenance remain separate from Workspace list order.
use super::workspace_columns_tests::{mutate, prepare, workspace, write};
use crate::{
    fleet_record_codec::FleetRecordHeader,
    fleet_storage_fixture::{FleetStorageFixture, NOW, TestError},
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use message_board::{ActorOrigin, Record, RecordTime, RepositoryRef, WorkspaceId};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real second-write key collision must roll back the first attachment in the same owner transaction."
)]
async fn late_attachment_key_failure_rolls_back_the_entire_multi_reference_transaction()
-> Result<(), TestError> {
    let fixture = seeded().await?;
    let failure = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let parent =
                    WorkspaceId::try_from("019f0000-0000-7000-8000-000000009e01".to_owned())
                        .map_err(StoreError::backend)?;
                let header = FleetRecordHeader {
                    time: RecordTime::LegacyUnknown,
                    origin: ActorOrigin {
                        installation: "019f0000-0000-7000-8000-000000009e31"
                            .to_owned()
                            .try_into()
                            .map_err(StoreError::backend)?,
                        machine: "first importer"
                            .to_owned()
                            .try_into()
                            .map_err(StoreError::backend)?,
                    },
                };
                let digest = fleet_admission::FleetImportDigest::new("ab".repeat(32))
                    .map_err(StoreError::backend)?;
                let id = durable_steps::Uuid::parse_str("019f0000-0000-7000-8000-000000009e32")
                    .map_err(StoreError::backend)?;
                let now = chrono::DateTime::from_timestamp_millis(NOW.unix_millis())
                    .ok_or_else(|| StoreError::backend("invalid clock"))?;
                for local in [false, true] {
                    let reference =
                        reference(local).map_err(|error| StoreError::backend(error.to_string()))?;
                    crate::fleet_repository_store::attach(
                        scope,
                        &parent,
                        &reference,
                        &header,
                        Some(&digest),
                        id,
                        now,
                    )
                    .await?;
                }
                Ok(())
            })
        })
        .await;
    assert!(failure.is_err());
    assert!(read(&fixture).await?.is_empty());
    let fixture = fixture.reopen().await?;
    assert!(read(&fixture).await?.is_empty());
    fixture.close().await?;
    Ok(())
}

fn reference(local: bool) -> Result<RepositoryRef, TestError> {
    Ok(if local {
        RepositoryRef::Local {
            service_id: "019f0000-0000-7000-8000-000000009e30"
                .to_owned()
                .try_into()?,
            common_directory: "/literal/repository/λ".to_owned().try_into()?,
        }
    } else {
        RepositoryRef::Origin {
            normalized_origin: "example.test/association".to_owned().try_into()?,
        }
    })
}
async fn seeded() -> Result<FleetStorageFixture, TestError> {
    let fixture = prepare().await?;
    let mut record = workspace(1)?;
    let Record::Workspace(value) = &mut record else {
        return Err("wrong fixture kind".into());
    };
    value.repositories.clear();
    write(&fixture, record, NOW.unix_millis()).await?;
    Ok(fixture)
}
async fn attach(
    fixture: &FleetStorageFixture,
    reference: RepositoryRef,
    id: &str,
    later: bool,
) -> Result<(), TestError> {
    let id = durable_steps::Uuid::parse_str(id)?;
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let header = FleetRecordHeader {
                    time: if later {
                        RecordTime::Router(
                            chrono::DateTime::from_timestamp_millis(NOW.unix_millis() + 17)
                                .ok_or_else(|| StoreError::backend("invalid clock"))?,
                        )
                    } else {
                        RecordTime::LegacyUnknown
                    },
                    origin: ActorOrigin {
                        installation: "019f0000-0000-7000-8000-000000009e31"
                            .to_owned()
                            .try_into()
                            .map_err(StoreError::backend)?,
                        machine: if later {
                            "later writer"
                        } else {
                            "first importer"
                        }
                        .to_owned()
                        .try_into()
                        .map_err(StoreError::backend)?,
                    },
                };
                let digest = fleet_admission::FleetImportDigest::new(
                    if later { "cd" } else { "ab" }.repeat(32),
                )
                .map_err(StoreError::backend)?;
                crate::fleet_repository_store::attach(
                    scope,
                    &WorkspaceId::try_from("019f0000-0000-7000-8000-000000009e01".to_owned())
                        .map_err(StoreError::backend)?,
                    &reference,
                    &header,
                    Some(&digest),
                    id,
                    chrono::DateTime::from_timestamp_millis(
                        NOW.unix_millis() + if later { 17 } else { 0 },
                    )
                    .ok_or_else(|| StoreError::backend("invalid clock"))?,
                )
                .await
            })
        })
        .await?;
    Ok(())
}
async fn read(fixture: &FleetStorageFixture) -> Result<Vec<RepositoryRef>, StoreError> {
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                crate::fleet_repository_store::read(
                    scope,
                    &WorkspaceId::try_from("019f0000-0000-7000-8000-000000009e01".to_owned())
                        .map_err(StoreError::backend)?,
                )
                .await
            })
        })
        .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The approved attachment fields must be physical typed columns, never a serialized repository reference."
)]
async fn repository_attachment_schema_exposes_typed_reference_and_original_header_columns()
-> Result<(), TestError> {
    let fixture = prepare().await?;
    let proof = async {
        let columns: Vec<(String, String, i64)> = fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>("PRAGMA table_info(fleet_repository_attachments)")
                        .fetch_all(scope.executor())
                        .await
                        .map_err(StoreError::backend)?
                        .into_iter()
                        .map(|row| {
                            Ok((
                                row.try_get("name").map_err(StoreError::backend)?,
                                row.try_get("type").map_err(StoreError::backend)?,
                                row.try_get("pk").map_err(StoreError::backend)?,
                            ))
                        })
                        .collect()
                })
            })
            .await?;
        for (name, kind, pk) in [
            ("attachment_id", "TEXT", 1),
            ("workspace_id", "TEXT", 0),
            ("reference_kind", "TEXT", 0),
            ("normalized_origin", "TEXT", 0),
            ("service_id", "TEXT", 0),
            ("common_directory", "TEXT", 0),
            ("router_time", "TEXT", 0),
            ("time_basis", "TEXT", 0),
            ("origin_installation", "TEXT", 0),
            ("origin_machine", "TEXT", 0),
            ("import_digest", "TEXT", 0),
            ("created_at", "INTEGER", 0),
            ("updated_at", "INTEGER", 0),
        ] {
            assert!(
                columns.contains(&(name.into(), kind.into(), pk)),
                "typed attachment column missing: {name}"
            );
        }
        assert!(
            !columns
                .iter()
                .any(|(name, _, _)| name == "repository_reference" || name == "payload"),
            "the typed reference must replace serialized JSON"
        );
        assert_eq!(
            columns.len(),
            13,
            "attachments have their own original header, not invented full record envelopes"
        );
        Ok::<_, TestError>(())
    }
    .await;
    let closed = fixture.close().await;
    closed?;
    proof
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The first association identity/header/digest/timestamps must remain unchanged after replay and owning reopen."
)]
async fn origin_and_local_replays_keep_the_first_association_header_and_ingestion_times()
-> Result<(), TestError> {
    for local in [false, true] {
        let fixture = seeded().await?;
        let reference = reference(local)?;
        attach(
            &fixture,
            reference.clone(),
            "019f0000-0000-7000-8000-000000009e32",
            false,
        )
        .await?;
        attach(
            &fixture,
            reference.clone(),
            "019f0000-0000-7000-8000-000000009e33",
            true,
        )
        .await?;
        assert_eq!(read(&fixture).await?, vec![reference.clone()]);
        let fixture = fixture.reopen().await?;
        assert_eq!(read(&fixture).await?, vec![reference]);
        let columns=fixture.object.transaction(|scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT attachment_id,router_time,time_basis,origin_machine,import_digest,created_at,updated_at FROM fleet_repository_attachments").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<String,_>("attachment_id").map_err(StoreError::backend)?,row.try_get::<Option<String>,_>("router_time").map_err(StoreError::backend)?,row.try_get::<String,_>("time_basis").map_err(StoreError::backend)?,row.try_get::<String,_>("origin_machine").map_err(StoreError::backend)?,row.try_get::<String,_>("import_digest").map_err(StoreError::backend)?,row.try_get::<i64,_>("created_at").map_err(StoreError::backend)?,row.try_get::<i64,_>("updated_at").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(
            columns,
            (
                "019f0000-0000-7000-8000-000000009e32".into(),
                None,
                "legacy_unknown".into(),
                "first importer".into(),
                "ab".repeat(32),
                NOW.unix_millis(),
                NOW.unix_millis()
            )
        );
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Only matching reference business keys replay; an unrelated primary-key collision must remain a real SQL failure."
)]
async fn reference_business_keys_are_distinct_and_primary_key_conflicts_are_not_swallowed()
-> Result<(), TestError> {
    let fixture = seeded().await?;
    let origin = reference(false)?;
    let local = reference(true)?;
    attach(
        &fixture,
        origin.clone(),
        "019f0000-0000-7000-8000-000000009e32",
        false,
    )
    .await?;
    assert!(
        attach(
            &fixture,
            local.clone(),
            "019f0000-0000-7000-8000-000000009e32",
            false
        )
        .await
        .is_err()
    );
    assert_eq!(read(&fixture).await?, vec![origin.clone()]);
    attach(
        &fixture,
        local.clone(),
        "019f0000-0000-7000-8000-000000009e33",
        false,
    )
    .await?;
    assert_eq!(
        read(&fixture)
            .await?
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([origin.clone(), local])
    );
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                crate::fleet_repository_store::detach(
                    scope,
                    &WorkspaceId::try_from("019f0000-0000-7000-8000-000000009e01".to_owned())
                        .map_err(StoreError::backend)?,
                    &origin,
                )
                .await
            })
        })
        .await?;
    assert_eq!(read(&fixture).await?, vec![reference(true)?]);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unknown tags/classes and fields outside the active repository variant must fail the real attachment decoder."
)]
async fn attachment_rejects_unknown_tags_physical_classes_and_partial_reference_variants()
-> Result<(), TestError> {
    for sql in [
        "UPDATE fleet_repository_attachments SET reference_kind='unknown'",
        "UPDATE fleet_repository_attachments SET reference_kind=x'6f726967696e'",
        "UPDATE fleet_repository_attachments SET reference_kind='local'",
        "UPDATE fleet_repository_attachments SET service_id='019f0000-0000-7000-8000-000000009e30'",
        "UPDATE fleet_repository_attachments SET normalized_origin='HTTPS://Example.test/repo'",
        "UPDATE fleet_repository_attachments SET normalized_origin=NULL",
        "UPDATE fleet_repository_attachments SET created_at=x'31'",
        "UPDATE fleet_repository_attachments SET time_basis='router'",
        "UPDATE fleet_repository_attachments SET import_digest=NULL",
        "UPDATE fleet_repository_attachments SET attachment_id='00000000-0000-4000-8000-000000000001'",
    ] {
        let fixture = seeded().await?;
        attach(
            &fixture,
            reference(false)?,
            "019f0000-0000-7000-8000-000000009e32",
            false,
        )
        .await?;
        mutate(&fixture, sql).await?;
        assert!(
            matches!(read(&fixture).await, Err(StoreError::Corrupt { .. })),
            "invalid attachment accepted: {sql}"
        );
        fixture.close().await?;
    }
    Ok(())
}
