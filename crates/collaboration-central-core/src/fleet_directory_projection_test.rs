//! Committed directory projections keep exact snapshots through reopen and reject corrupt fields.
use super::directory_columns_tests::{PROJECT, seeded};
use crate::fleet_storage_fixture::TestError;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use sqlx_core::{query::query, row::Row};
fn projection(
    lifecycle: fleet_project_lease::DatabaseLifecycle,
    epoch: u64,
    change: u64,
) -> Result<crate::lease_projection::LeaseProjection, TestError> {
    use fleet_project_lease::{
        DatabaseLifecycle, RegistryGrant, RegistryRevision, RegistrySnapshot,
    };
    use fleet_protocol::{Epoch, SyncTime};
    let epoch = Epoch::new(epoch)?;
    let held = matches!(lifecycle, DatabaseLifecycle::Registered { .. });
    crate::lease_projection::LeaseProjection::from_snapshot(
        RegistrySnapshot {
            database: PROJECT.parse()?,
            revision: RegistryRevision {
                epoch,
                change: std::num::NonZeroU64::new(change).ok_or("zero fixture change")?,
            },
            changed_at: SyncTime::from_unix_millis(17),
            lifecycle,
            grant: if held {
                Some(RegistryGrant {
                    epoch,
                    holder: "019f0000-0000-7000-8000-000000009f14".parse()?,
                    expires_at: SyncTime::from_unix_millis(31),
                })
            } else {
                None
            },
        },
        if held {
            Some(
                "019f0000-0000-7000-8000-000000009f13"
                    .to_owned()
                    .try_into()?,
            )
        } else {
            None
        },
        crate::RegistryIncarnation::new(u64::MAX)?,
    )
    .map_err(Into::into)
}

async fn apply(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
    projection: crate::lease_projection::LeaseProjection,
) -> Result<crate::lease_projection::LeaseProjectionResult, StoreError> {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                crate::fleet_lease_projection_step::apply_projection(
                    scope,
                    &projection,
                    chrono::DateTime::from_timestamp_millis(
                        crate::fleet_storage_fixture::NOW.unix_millis() + 29,
                    )
                    .ok_or_else(|| StoreError::backend("invalid fixture time"))?,
                )
                .await
            })
        })
        .await
}
async fn lease(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
) -> Result<Option<crate::lease_projection::LeaseProjection>, StoreError> {
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                crate::fleet_lease_columns::decode(
                    crate::generated_fleet_lease::read_directory_lease(scope, PROJECT.into())
                        .await
                        .map_err(StoreError::backend)?
                        .ok_or_else(|| StoreError::backend("directory missing"))?,
                )
            })
        })
        .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Every Registry lifecycle and full u64 projection counter must retain exact snapshots and digest across owning reopen."
)]
async fn directory_lease_variants_and_full_u64_counters_survive_reopen() -> Result<(), TestError> {
    use crate::lease_projection::LeaseProjectionResult;
    use fleet_project_lease::{DatabaseLifecycle, ImportOrigin};
    use fleet_protocol::{CommitSequence, Epoch, Origin, ReplicationPosition, SyncTime};
    let digest = fleet_protocol::ImportDigest::new("ab".repeat(32))?;
    for lifecycle in [
        DatabaseLifecycle::Registered {
            origin: Origin::Empty,
            imported_from: None,
        },
        DatabaseLifecycle::Registered {
            origin: Origin::Seeded {
                position: ReplicationPosition::new(
                    Epoch::new(u64::MAX)?,
                    CommitSequence::new(u64::MAX),
                ),
            },
            imported_from: Some(ImportOrigin {
                digest: digest.clone(),
            }),
        },
        DatabaseLifecycle::Unheld {
            import_digest: digest,
        },
        DatabaseLifecycle::Retiring {
            attempt: "019f0000-0000-7000-8000-000000009f15".parse()?,
        },
        DatabaseLifecycle::Retired {
            at: SyncTime::from_unix_millis(i64::MIN),
        },
    ] {
        let fixture = seeded().await?;
        assert_eq!(lease(&fixture).await?, None);
        let offered = projection(lifecycle, u64::MAX, u64::MAX)?;
        assert_eq!(
            apply(&fixture, offered.clone()).await?,
            LeaseProjectionResult::Applied
        );
        assert_eq!(lease(&fixture).await?, Some(offered.clone()));
        assert_eq!(
            apply(&fixture, offered.clone()).await?,
            LeaseProjectionResult::Duplicate
        );
        let fixture = fixture.reopen().await?;
        assert_eq!(lease(&fixture).await?, Some(offered));
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Older and conflicting projections must preserve the previously committed fields."
)]
async fn directory_lease_older_conflicting_and_newer_revisions_keep_original_rules()
-> Result<(), TestError> {
    use crate::lease_projection::LeaseProjectionResult;
    use fleet_project_lease::DatabaseLifecycle;
    let fixture = seeded().await?;
    let original = projection(
        DatabaseLifecycle::Retiring {
            attempt: "019f0000-0000-7000-8000-000000009f15".parse()?,
        },
        9,
        2,
    )?;
    assert_eq!(
        apply(&fixture, original.clone()).await?,
        LeaseProjectionResult::Applied
    );
    let older = projection(
        DatabaseLifecycle::Retired {
            at: crate::fleet_storage_fixture::NOW,
        },
        9,
        1,
    )?;
    assert_eq!(
        apply(&fixture, older).await?,
        LeaseProjectionResult::Older {
            stored: original.version()
        }
    );
    let conflict = projection(
        DatabaseLifecycle::Retired {
            at: crate::fleet_storage_fixture::NOW,
        },
        9,
        2,
    )?;
    assert_eq!(
        apply(&fixture, conflict).await?,
        LeaseProjectionResult::Conflict {
            stored: original.version()
        }
    );
    assert_eq!(lease(&fixture).await?, Some(original));
    let newer = projection(
        DatabaseLifecycle::Retired {
            at: crate::fleet_storage_fixture::NOW,
        },
        9,
        3,
    )?;
    assert_eq!(
        apply(&fixture, newer.clone()).await?,
        LeaseProjectionResult::Applied
    );
    let fixture = fixture.reopen().await?;
    assert_eq!(lease(&fixture).await?, Some(newer));
    fixture.close().await?;
    Ok(())
}

async fn copy(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
) -> Result<Option<crate::CopyDirectorySummary>, StoreError> {
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                crate::fleet_copy_projection::decode(
                    crate::generated_fleet_record_reads::read_directory_summary(
                        scope,
                        PROJECT.into(),
                    )
                    .await
                    .map_err(StoreError::backend)?
                    .ok_or_else(|| StoreError::backend("directory missing"))?,
                )
            })
        })
        .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Copy's full u64 position, precise or unknown domain time and own ACK time must survive owning reopen without substituting ingestion time."
)]
async fn directory_copy_summary_keeps_full_position_domain_time_and_ack_across_reopen()
-> Result<(), TestError> {
    use crate::fleet_storage_fixture::NOW;
    use board_sync_protocol::BoardAnnotation;
    use fleet_protocol::{CommitSequence, Epoch, ReplicationPosition, SyncTime};
    use message_board::RecordTime;
    for offered in [
        RecordTime::LegacyUnknown,
        RecordTime::Router("2026-11-04T10:11:12.123456789Z".parse()?),
    ] {
        let fixture = seeded().await?;
        assert_eq!(copy(&fixture).await?, None);
        let latest = offered;
        fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    crate::generated_fleet_record_writes::store_directory_summary(
                        scope,
                        Some(u64::MAX.to_string()),
                        Some(u64::MAX.to_string()),
                        Some(i64::MIN.to_string()),
                        Some(
                            match latest {
                                RecordTime::Router(_) => "router",
                                RecordTime::LegacyUnknown => "legacy_unknown",
                            }
                            .into(),
                        ),
                        latest.as_router_time().map(|time| time.to_rfc3339()),
                        (NOW.unix_millis() + 17).to_string(),
                        PROJECT.into(),
                    )
                    .await
                    .map_err(StoreError::backend)
                })
            })
            .await?;
        let expected = crate::CopyDirectorySummary {
            database: PROJECT.parse()?,
            summary: BoardAnnotation::ProjectSummary {
                latest_record_at: offered,
            },
            position: ReplicationPosition::new(
                Epoch::new(u64::MAX)?,
                CommitSequence::new(u64::MAX),
            ),
            acked_at: SyncTime::from_unix_millis(i64::MIN),
        };
        assert_eq!(copy(&fixture).await?, Some(expected.clone()));
        let fixture = fixture.reopen().await?;
        assert_eq!(copy(&fixture).await?, Some(expected));
        let times = fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    let row = query::<sqlx_turso::Turso>(
                        "SELECT created_at,updated_at FROM fleet_project_directory",
                    )
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                    Ok((
                        row.try_get::<i64, _>("created_at")
                            .map_err(StoreError::backend)?,
                        row.try_get::<i64, _>("updated_at")
                            .map_err(StoreError::backend)?,
                    ))
                })
            })
            .await?;
        assert_eq!(times, (NOW.unix_millis(), NOW.unix_millis() + 17));
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unknown tags, mismatched digests, inactive union fields and malformed physical values must be refused by the real lease decoder."
)]
async fn directory_lease_rejects_unknown_partial_noncanonical_and_wrong_class_fields()
-> Result<(), TestError> {
    use super::workspace_columns_tests::mutate;
    use fleet_project_lease::DatabaseLifecycle;
    for sql in [
        "UPDATE fleet_project_directory SET lease_epoch='01'",
        "UPDATE fleet_project_directory SET lease_epoch='0'",
        "UPDATE fleet_project_directory SET lease_epoch='18446744073709551616'",
        "UPDATE fleet_project_directory SET lease_epoch=x'39'",
        "UPDATE fleet_project_directory SET lease_change='0'",
        "UPDATE fleet_project_directory SET lease_incarnation='0'",
        "UPDATE fleet_project_directory SET lease_changed_at=x'3137'",
        "UPDATE fleet_project_directory SET lease_lifecycle_kind='unknown'",
        "UPDATE fleet_project_directory SET lease_lifecycle_kind=x'7265746972696e67'",
        "UPDATE fleet_project_directory SET lease_origin_kind='empty'",
        "UPDATE fleet_project_directory SET lease_origin_sequence='1'",
        "UPDATE fleet_project_directory SET lease_import_digest='bad'",
        "UPDATE fleet_project_directory SET lease_retiring_attempt_id=NULL",
        "UPDATE fleet_project_directory SET lease_retiring_attempt_id='019f0000-0000-4000-8000-000000009f15'",
        "UPDATE fleet_project_directory SET lease_holder_service_id='019f0000-0000-7000-8000-000000009f13'",
        "UPDATE fleet_project_directory SET lease_expires_at=3",
        "UPDATE fleet_project_directory SET lease_retired_at=3",
        "UPDATE fleet_project_directory SET lease_digest='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
        "UPDATE fleet_project_directory SET lease_digest=NULL",
    ] {
        let fixture = seeded().await?;
        apply(
            &fixture,
            projection(
                DatabaseLifecycle::Retiring {
                    attempt: "019f0000-0000-7000-8000-000000009f15".parse()?,
                },
                9,
                1,
            )?,
        )
        .await?;
        mutate(&fixture, sql).await?;
        assert!(lease(&fixture).await.is_err(), "{sql}");
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Missing or malformed Copy fields must fail closed instead of turning into an invented unreported state."
)]
async fn directory_copy_rejects_partial_noncanonical_time_and_wrong_physical_classes()
-> Result<(), TestError> {
    use super::workspace_columns_tests::mutate;
    for sql in [
        "UPDATE fleet_project_directory SET copy_epoch='1'",
        "UPDATE fleet_project_directory SET copy_epoch='1',copy_sequence='1',copy_acked_at=17,copy_latest_time_basis='unknown'",
        "UPDATE fleet_project_directory SET copy_epoch='01',copy_sequence='1',copy_acked_at=17,copy_latest_time_basis='legacy_unknown'",
        "UPDATE fleet_project_directory SET copy_epoch='0',copy_sequence='1',copy_acked_at=17,copy_latest_time_basis='legacy_unknown'",
        "UPDATE fleet_project_directory SET copy_epoch=x'31',copy_sequence='1',copy_acked_at=17,copy_latest_time_basis='legacy_unknown'",
        "UPDATE fleet_project_directory SET copy_epoch='1',copy_sequence='18446744073709551616',copy_acked_at=17,copy_latest_time_basis='legacy_unknown'",
        "UPDATE fleet_project_directory SET copy_epoch='1',copy_sequence='1',copy_acked_at=x'3137',copy_latest_time_basis='legacy_unknown'",
        "UPDATE fleet_project_directory SET copy_epoch='1',copy_sequence='1',copy_acked_at=17,copy_latest_time_basis='router'",
        "UPDATE fleet_project_directory SET copy_epoch='1',copy_sequence='1',copy_acked_at=17,copy_latest_time_basis='router',copy_latest_record_at='invalid'",
        "UPDATE fleet_project_directory SET copy_epoch='1',copy_sequence='1',copy_acked_at=17,copy_latest_time_basis='legacy_unknown',copy_latest_record_at='2026-11-04T10:11:12Z'",
    ] {
        let fixture = seeded().await?;
        mutate(&fixture, sql).await?;
        assert!(copy(&fixture).await.is_err(), "{sql}");
        fixture.close().await?;
    }
    Ok(())
}
