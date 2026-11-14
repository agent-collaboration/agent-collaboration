use super::support::*;
use serde_json::json;
use std::time::Duration;
type PersistentStoreFiles = Vec<(&'static str, Option<Vec<std::path::PathBuf>>)>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Route and stored-state assertions retain B2 behavior while fixture errors propagate."
)]
#[tokio::test]
async fn provision_repeat_restart_and_predecessor_renewal_preserve_existing_modern_rows() -> Result
{
    let mut f = Fixture::new().await?;
    f.seed_machine().await?;
    f.apply(A,"INSERT INTO workspaces(workspace_id,name,state,created_at,updated_at) VALUES (?,'existing workspace','active','2030-03-17T17:46:40Z','2030-03-17T17:46:40Z')",vec![text(WORKSPACE)]).await?;
    let business = (
        f.rows(A, "SELECT * FROM workspaces ORDER BY workspace_id", vec![])
            .await?,
        f.rows(A, "SELECT * FROM machines ORDER BY machine_id", vec![])
            .await?,
        f.rows(
            A,
            "SELECT * FROM machine_credentials ORDER BY credential_id",
            vec![],
        )
        .await?,
    );
    assert_eq!(
        f.digest(A).await?.status,
        404,
        "an unknown org has no stamp"
    );
    let statement = signed::stamp(A, signed::NOW_SECONDS)?;
    let first = f.provision(A, &statement).await?;
    let before = f.snapshot().await?;
    let repeat = f.provision(A, &statement).await?;
    assert_eq!(first.status, 200);
    assert!(first.body == br#"{"result":"provisioned"}"#);
    assert_eq!(repeat.status, 200);
    assert!(repeat.body == first.body);
    assert!(
        f.snapshot().await? == before,
        "repeats must not rewrite stamp, record or timestamps"
    );
    assert_eq!(before.0.len(), 1);
    assert_eq!(before.1.len(), 1);
    let record = before.1.first().ok_or("missing record")?;
    let Some(central_storage::SqlValue::Text(record_id)) = record.first() else {
        return Err("record id missing".into());
    };
    assert_eq!(uuid::Uuid::parse_str(record_id)?.get_version_num(), 7);
    assert!(
        record.get(1) == Some(&text(A))
            && record.get(2) == Some(&text("organizationProvisioned"))
            && record.get(3) == Some(&text("signInProvisioning"))
            && record.get(4) == Some(&central_storage::SqlValue::Null)
            && record.get(5) == Some(&central_storage::SqlValue::Null)
            && record.get(6) == Some(&text(A))
            && record.get(7) == Some(&number(signed::NOW_SECONDS * 1000)?),
        "setup record invents neither a person nor permission"
    );
    let digest = f.digest(A).await?.value()?;
    assert_eq!(
        digest,
        json!({"digest":signed::digest(&statement),"expiresAt":signed::NOW_SECONDS-60+signed::LIFETIME_SECONDS,"signingKeyId":"stamp-a"})
    );
    f.reopen().await?;
    assert_eq!(f.provision(A, &statement).await?.status, 200);
    assert!(f.snapshot().await? == before);
    let renewal = signed::renewal(A, &statement, signed::NOW_SECONDS, 10)?;
    assert_eq!(f.provision(A, &renewal).await?.status, 200);
    let renewed = f.snapshot().await?;
    assert_eq!((renewed.0.len(), renewed.1.len()), (1, 2));
    assert_eq!(f.provision(A, &renewal).await?.status, 200);
    let stale = signed::renewal(A, &statement, signed::NOW_SECONDS, 20)?;
    assert_eq!(
        f.provision(A, &stale).await?.status,
        409,
        "renewal must name the current digest exactly"
    );
    assert!(f.snapshot().await? == renewed);
    f.clock
        .advance(Duration::from_secs(signed::LIFETIME_SECONDS.try_into()?))?;
    let current = f.clock_time();
    assert_eq!(
        f.provision(A, &signed::stamp(A, current)?).await?.status,
        409,
        "an expired stored statement needs an exact predecessor"
    );
    let replacement = signed::renewal(A, &renewal, current, 0)?;
    assert_eq!(
        f.provision(A, &replacement).await?.status,
        200,
        "an exact predecessor permits replacement of an expired stamp"
    );
    assert_eq!(f.snapshot().await?.1.len(), 3);
    assert!(
        (
            f.rows(A, "SELECT * FROM workspaces ORDER BY workspace_id", vec![])
                .await?,
            f.rows(A, "SELECT * FROM machines ORDER BY machine_id", vec![])
                .await?,
            f.rows(
                A,
                "SELECT * FROM machine_credentials ORDER BY credential_id",
                vec![]
            )
            .await?,
        ) == business,
        "provisioning preserves every modern workspace, machine and credential column"
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Invalid real signatures and profiles must refuse without storage writes."
)]
#[tokio::test]
async fn unstamped_routes_and_forged_foreign_unpublished_expired_profiles_fail_closed() -> Result {
    let f = Fixture::new().await?;
    assert_eq!(f.digest(A).await?.status, 404);
    assert_eq!(
        f.post(REVISIONS, &organization_floor(A, 1, "active"))
            .await?
            .status,
        503
    );
    let person = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let lookup = json!({"organizationId":A,"workspaceId":WORKSPACE});
    assert_eq!(
        f.person(
            signed::LOOKUP,
            &lookup,
            &person,
            &signed::lookup_intent(A, WORKSPACE),
            &signed::fresh_request_id(signed::NOW_SECONDS)?
        )
        .await?
        .status,
        503
    );
    let valid = signed::stamp(A, signed::NOW_SECONDS)?;
    for (statement, status) in [
        (signed::tamper_signature(&valid)?, 422),
        (signed::stamp(B, signed::NOW_SECONDS)?, 403),
        (
            signed::sign(
                &json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"unpublished"}),
                &signed::claims(A, signed::NOW_SECONDS),
                9,
            )?,
            422,
        ),
        (
            signed::stamp(A, signed::NOW_SECONDS - signed::LIFETIME_SECONDS)?,
            422,
        ),
    ] {
        assert_eq!(f.provision(A, &statement).await?.status, status);
    }
    for (field, value) in [
        ("iss", json!("https://wrong.example.invalid")),
        ("aud", json!("wrong")),
        ("purpose", json!("not-a-stamp")),
        ("version", json!(2)),
    ] {
        let mut claims = signed::claims(A, signed::NOW_SECONDS);
        signed::set_claim(&mut claims, field, value)?;
        assert_eq!(
            f.provision(A, &signed::sign(&signed::stamp_header(), &claims, 7)?)
                .await?
                .status,
            422
        );
    }
    for header in [
        json!({"typ":"wrong+jwt","alg":"EdDSA","kid":"stamp-a"}),
        json!({"typ":"fleet-organization-stamp+jwt","alg":"ES256","kid":"stamp-a"}),
    ] {
        assert_eq!(
            f.provision(
                A,
                &signed::sign(&header, &signed::claims(A, signed::NOW_SECONDS), 7)?
            )
            .await?
            .status,
            422
        );
    }
    let snapshot = f.snapshot().await?;
    assert!(
        snapshot.0.is_empty() && snapshot.1.is_empty(),
        "no invalid statement writes an identity or record"
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Cold tamper and retired-key cases compare unchanged owning store rows."
)]
#[tokio::test]
async fn cold_tampered_foreign_and_retired_stamps_refuse_without_rewriting_stored_rows() -> Result {
    for damage in ["signature", "foreign", "retired"] {
        let mut f = Fixture::new().await?;
        let original = if damage == "retired" {
            signed::stamp_under_second_key(A, signed::NOW_SECONDS)?
        } else {
            signed::stamp(A, signed::NOW_SECONDS)?
        };
        assert_eq!(f.provision(A, &original).await?.status, 200);
        match damage {
            "signature" => {
                f.apply(
                    A,
                    "UPDATE organization_identity SET organization_statement=?",
                    vec![text(signed::tamper_signature(&original)?)],
                )
                .await?
            }
            "foreign" => {
                f.apply(
                    A,
                    "UPDATE organization_identity SET organization_statement=?",
                    vec![text(signed::stamp(B, signed::NOW_SECONDS)?)],
                )
                .await?
            }
            _ => f.keys.retire("stamp-b")?,
        }
        let before = f.snapshot().await?;
        let root = f.root.path().to_owned();
        let persistent_store_files = || -> Result<PersistentStoreFiles> {
            let mut inventory = Vec::new();
            for directory in ["central", "workspace-stores"] {
                let files = match std::fs::read_dir(root.join(directory)) {
                    Ok(entries) => {
                        let mut files = Vec::new();
                        for entry in entries {
                            let entry = entry?;
                            if entry.file_type()?.is_file()
                                && entry
                                    .path()
                                    .extension()
                                    .and_then(|extension| extension.to_str())
                                    == Some("sqlite")
                            {
                                files.push(
                                    std::path::PathBuf::from(directory).join(entry.file_name()),
                                );
                            }
                        }
                        files.sort();
                        Some(files)
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error.into()),
                };
                inventory.push((directory, files));
            }
            Ok(inventory)
        };
        let files_before = if damage == "foreign" {
            Some(persistent_store_files()?)
        } else {
            None
        };
        f.reopen().await?;
        assert_eq!(f.digest(A).await?.status, 503);
        assert_eq!(
            f.provision(A, &signed::stamp(A, signed::NOW_SECONDS)?)
                .await?
                .status,
            409
        );
        if damage == "foreign" {
            assert!(
                before.0.first().and_then(|row| row.first()) == Some(&text(A)),
                "the damaged store must keep A's row id while holding B's compact",
            );
            assert_eq!(
                f.provision(A, &signed::stamp(B, signed::NOW_SECONDS)?)
                    .await?
                    .status,
                403,
                "B's valid compact offered to A is refused even when A already stores B's compact",
            );
            assert!(
                f.snapshot().await? == before,
                "refusing the foreign offered compact must leave the damaged stored state unchanged",
            );
        }
        assert_eq!(
            f.post(REVISIONS, &organization_floor(A, 1, "active"))
                .await?
                .status,
            503
        );
        assert!(
            f.snapshot().await? == before,
            "refusing cold trust never rewrites the stamp or records"
        );
        if damage == "foreign" {
            // Modern sign-in owns org and workspace SQLite stores. This adapts
            // the old Fleet/Copy file oracle without claiming a legacy Copy run.
            // None preserves directory absence; WAL and temporary files are excluded.
            assert_eq!(
                Some(persistent_store_files()?),
                files_before,
                "refused calls open no new org or workspace database; absent directories stay absent",
            );
        }
        f.close().await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The real fetched key adapter must honor the retained 300-second boundary."
)]
#[tokio::test]
async fn a_warm_key_cache_refuses_a_retired_key_once_its_refresh_bound_passes() -> Result {
    let f = Fixture::new().await?;
    let first = signed::stamp_under_second_key(A, signed::NOW_SECONDS)?;
    assert_eq!(f.provision(A, &first).await?.status, 200);
    let warmed = f.keys.fetches();
    f.keys.retire("stamp-b")?;
    f.clock.advance(Duration::from_millis(299_999))?;
    let inside = f
        .provision(
            A,
            &signed::stamp_under_second_key_at(A, signed::NOW_SECONDS, 1)?,
        )
        .await?;
    let inside_fetches = f.keys.fetches();
    f.clock.advance(Duration::from_millis(1))?;
    let past = f
        .provision(
            A,
            &signed::stamp_under_second_key_at(A, signed::NOW_SECONDS, 2)?,
        )
        .await?;
    let boundary_fetches = f.keys.fetches();
    assert_eq!(
        inside.status, 200,
        "inside the bound the cached key still verifies"
    );
    assert_eq!(
        past.status, 422,
        "from the bound on, the retired key is refused"
    );
    assert_eq!(
        (warmed, inside_fetches, boundary_fetches),
        (1, 1, 3),
        "stored standing refresh plus incoming key's one forced refresh"
    );
    assert_eq!(
        f.provision(A, &signed::stamp(A, signed::NOW_SECONDS)?)
            .await?
            .status,
        409
    );
    assert_eq!(
        f.provision(B, &signed::stamp(B, signed::NOW_SECONDS)?)
            .await?
            .status,
        200
    );
    let stored = f.snapshot().await?;
    assert_eq!((stored.0.len(), stored.1.len()), (1, 1));
    assert!(
        stored.0.first().and_then(|row| row.get(1)) == Some(&text(first)),
        "A keeps its first statement"
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Concurrent real routes must retain a single identity and one record per committed renewal."
)]
#[tokio::test]
async fn concurrent_first_statements_and_competing_renewals_keep_one_winner() -> Result {
    let f = Fixture::new().await?;
    let first = signed::stamp(A, signed::NOW_SECONDS)?;
    let competing = signed::first_statement(A, signed::NOW_SECONDS, 1)?;
    let (one, two) = tokio::join!(f.provision(A, &first), f.provision(A, &competing));
    assert_eq!((one?.status, two?.status), (200, 200));
    let snapshot = f.snapshot().await?;
    assert_eq!((snapshot.0.len(), snapshot.1.len()), (1, 1));
    let stored = snapshot
        .0
        .first()
        .and_then(|row| row.get(1))
        .ok_or("stamp missing")?;
    let central_storage::SqlValue::Text(stored) = stored else {
        return Err("stamp not text".into());
    };
    let renewal_one = signed::renewal(A, stored, signed::NOW_SECONDS, 10)?;
    let renewal_two = signed::renewal(A, stored, signed::NOW_SECONDS, 20)?;
    let (one, two) = tokio::join!(f.provision(A, &renewal_one), f.provision(A, &renewal_two));
    let mut statuses = [one?.status, two?.status];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let after = f.snapshot().await?;
    assert_eq!((after.0.len(), after.1.len()), (1, 2));
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A real late SQLite record failure must roll back the preceding stamp insert."
)]
#[tokio::test]
async fn a_late_record_failure_rolls_back_the_stamp_and_retry_commits_both() -> Result {
    let f = Fixture::new().await?;
    f.apply(A,"CREATE TRIGGER signin_record_failure BEFORE INSERT ON organization_change_records BEGIN SELECT RAISE(ABORT,'fixture organization record failure'); END",vec![]).await?;
    let statement = signed::stamp(A, signed::NOW_SECONDS)?;
    assert_eq!(f.provision(A, &statement).await?.status, 503);
    assert_eq!(
        f.digest(A).await?.status,
        404,
        "a failed change record must leave no provisioned identity"
    );
    let failed = f.snapshot().await?;
    assert!(
        failed.0.is_empty() && failed.1.is_empty(),
        "stamp and change record roll back together"
    );
    f.apply(A, "DROP TRIGGER signin_record_failure", vec![])
        .await?;
    assert_eq!(f.provision(A, &statement).await?.status, 200);
    let retried = f.snapshot().await?;
    assert_eq!((retried.0.len(), retried.1.len()), (1, 1));
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Exact HTTP and owning-reopen assertions preserve lost-answer idempotency while fixture errors propagate."
)]
#[tokio::test]
async fn lost_provisioning_answer_reopens_with_exact_stamp_and_one_record() -> Result {
    let mut f = Fixture::new().await?;
    let statement = signed::stamp(A, signed::NOW_SECONDS)?;
    f.runtime(A).await?.lose_next_provisioning_reply();
    assert_eq!(f.provision(A, &statement).await?.status, 503);
    let before = f.snapshot().await?;
    assert_eq!((before.0.len(), before.1.len()), (1, 1));
    f.reopen().await?;
    assert_eq!(f.provision(A, &statement).await?.status, 200);
    assert!(
        f.snapshot().await? == before,
        "exact retry after owning reopen must retain stamp and record identities/timestamps"
    );
    f.close().await
}
