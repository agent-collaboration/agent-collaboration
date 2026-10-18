use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, StorageCorruption, holder_fixtures::*,
    import_fixtures::*,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
fn rehash(fixture: &mut ImportedProjectFixture) -> ConformanceResult<()> {
    let first = fixture.pages.first().ok_or("first")?;
    let mut digest = ImportDigestBuilder::new(&ImportSnapshotHeader::from(&first.page))?;
    for page in &fixture.pages {
        for row in &page.page.rows {
            digest.append_row(&page.page.table, row)?;
        }
    }
    let digest = digest.finish(&fixture.summary)?;
    for page in &mut fixture.pages {
        page.digest = digest.clone();
    }
    Ok(())
}
async fn staging(client: &ConformanceClient) -> ConformanceResult<()> {
    let fixture = ImportedProjectFixture::new()?;
    let first = fixture.pages.first().ok_or("first")?;
    let last = fixture.pages.last().ok_or("last")?;
    let staged = ProjectImportAnswer::Staged {
        digest: fixture.digest()?,
        index: 0,
    };
    equal(
        client.import_project(&fixture.project, first).await?,
        staged.clone(),
    )?;
    equal(
        fixture.snapshot(client).await?,
        SessionAnswer::Refused(SessionRefusal::NotInitialized),
    )?;
    let mut repeat = first.clone();
    repeat.page.as_of += chrono::TimeDelta::hours(1);
    equal(
        client.import_project(&fixture.project, &repeat).await?,
        staged,
    )?;
    let mut wrong = last.clone();
    wrong.index = 9;
    equal(
        client.import_project(&fixture.project, &wrong).await?,
        ProjectImportAnswer::Refused(ProjectImportRefusal::UnexpectedIndex { expected: 1 }),
    )?;
    let mut changed = first.clone();
    changed.page.rows.clear();
    equal(
        client.import_project(&fixture.project, &changed).await?,
        ProjectImportAnswer::Refused(ProjectImportRefusal::UnexpectedIndex { expected: 1 }),
    )?;
    equal(
        client.import_project(&fixture.project, last).await?,
        ProjectImportAnswer::Imported {
            head: position(1, 3)?,
        },
    )?;
    equal(
        client.import_project(&fixture.project, first).await?,
        ProjectImportAnswer::AlreadyImported,
    )?;
    let mut corrupted = ImportedProjectFixture::new()?;
    for page in &mut corrupted.pages {
        page.digest = ImportDigest::new("f".repeat(64))?;
    }
    let first = corrupted.pages.first().ok_or("first")?;
    let last = corrupted.pages.last().ok_or("last")?;
    equal(
        client.import_project(&corrupted.project, first).await?,
        ProjectImportAnswer::Staged {
            digest: first.digest.clone(),
            index: 0,
        },
    )?;
    check(
        matches!(
            client.import_project(&corrupted.project, last).await?,
            ProjectImportAnswer::Refused(ProjectImportRefusal::DigestMismatch { .. })
        ),
        "last page validates digest and rolls back publication",
    )?;
    equal(
        corrupted.snapshot(client).await?,
        SessionAnswer::Refused(SessionRefusal::NotInitialized),
    )?;
    // A different digest at index zero discards the unfinished import.
    rehash(&mut corrupted)?;
    corrupted.upload(client).await?;
    let mut bad_schema = ImportedProjectFixture::new()?;
    for page in &mut bad_schema.pages {
        page.page.schema.fingerprint = "mismatched-schema".into();
    }
    rehash(&mut bad_schema)?;
    equal(
        client
            .import_project(
                &bad_schema.project,
                bad_schema.pages.first().ok_or("first")?,
            )
            .await?,
        ProjectImportAnswer::Staged {
            digest: bad_schema.digest()?,
            index: 0,
        },
    )?;
    equal(
        client
            .import_project(&bad_schema.project, bad_schema.pages.last().ok_or("last")?)
            .await?,
        ProjectImportAnswer::Refused(ProjectImportRefusal::SchemaMismatch),
    )?;
    equal(
        bad_schema.snapshot(client).await?,
        SessionAnswer::Refused(SessionRefusal::NotInitialized),
    )?;
    let mut unsupported = ImportedProjectFixture::new()?;
    for page in &mut unsupported.pages {
        page.page.schema_steps = vec!["PRAGMA user_version=99".into()];
    }
    rehash(&mut unsupported)?;
    let _ = client
        .import_project(
            &unsupported.project,
            unsupported.pages.first().ok_or("first")?,
        )
        .await?;
    equal(
        client
            .import_project(
                &unsupported.project,
                unsupported.pages.last().ok_or("last")?,
            )
            .await?,
        ProjectImportAnswer::Refused(ProjectImportRefusal::SchemaUnsupported {
            form: SchemaStatementForm::Pragma,
        }),
    )?;
    let full = ImportedProjectFixture::new()?;
    let _ = client
        .import_project(&full.project, full.pages.first().ok_or("first")?)
        .await?;
    client
        .control(ControlRequest::CopyFull {
            address: ObjectAddress::Copy(full.project.clone()),
            after_writes: 2,
        })
        .await?;
    equal(
        client
            .import_project(&full.project, full.pages.last().ok_or("last")?)
            .await?,
        ProjectImportAnswer::Refused(ProjectImportRefusal::CentralCopyFull),
    )?;
    equal(
        full.snapshot(client).await?,
        SessionAnswer::Refused(SessionRefusal::NotInitialized),
    )?;
    equal(
        client
            .import_project(&full.project, full.pages.last().ok_or("last")?)
            .await?,
        ProjectImportAnswer::Imported {
            head: position(1, 3)?,
        },
    )?;
    Ok(())
}
async fn schema_and_restore(client: &ConformanceClient) -> ConformanceResult<()> {
    let fixture = ImportedProjectFixture::new()?;
    crate::fleet_directory_fixture::import_identity(client, &fixture.project).await?;
    fixture.upload(client).await?;
    let unknown = HolderFixture::enroll(client, "unknown-schema").await?;
    let result = client
        .call(
            ObjectAddress::Registry,
            WireCall::Registry(RegistryCall::Claim {
                attempt: attempt()?,
                project: fixture.project.clone(),
                claimant: unknown.installation,
            }),
        )
        .await?;
    equal(
        result,
        WireResult::Registry(RegistryResult::Claim(SessionAnswer::Refused(
            SessionRefusal::SchemaTooNew {
                store: fixture.state(client).await?.schema,
                supported: None,
            },
        ))),
    )?;
    let a = HolderFixture::enroll(client, "supported-A").await?;
    let (mut a_socket, _) = a.connect_supported(client, 0).await?;
    let failed_attempt = attempt()?;
    let failure = SessionAnswer::Refused(SessionRefusal::SchemaTooNew {
        store: fixture.state(client).await?.schema,
        supported: Some(0),
    });
    equal(
        a_socket
            .request(claim_request(&fixture.project, failed_attempt.clone()))
            .await?,
        failure.clone(),
    )?;
    equal(fixture.state(client).await?.fence, epoch(1)?)?;
    a_socket.close().await?;
    let (mut a_socket, _) = a.connect(client).await?;
    equal(
        a_socket
            .request(claim_request(&fixture.project, failed_attempt))
            .await?,
        failure,
    )?;
    check(
        matches!(a_socket.request(claim_request(&fixture.project,attempt()?)).await?,SessionAnswer::Claimed{epoch,..} if epoch.value()==2),
        "new supported claim grants epoch 2",
    )?;
    client
        .control(ControlRequest::Corrupt {
            address: ObjectAddress::Registry,
            mutation: StorageCorruption::RestoreUnheldRegistry {
                unheld: fixture.unheld()?,
                as_of: time()?,
            },
        })
        .await?;
    let b = HolderFixture::enroll(client, "restore-B").await?;
    let (mut b_socket, _) = b.connect(client).await?;
    equal(
        b_socket
            .request(claim_request(&fixture.project, attempt()?))
            .await?,
        SessionAnswer::Refused(SessionRefusal::AlreadyHeld {
            holder: a.installation.clone(),
        }),
    )?;
    let state = fixture.state(client).await?;
    equal(state.fence, epoch(2)?)?;
    equal(state.holder, Some(a.installation))?;
    equal(
        b_socket
            .request(SessionRequest::Piece(piece(
                &fixture.project,
                position(1, 3)?,
                position(2, 1)?,
                vec![rows(1, vec![row(4, "forged")])],
            )?))
            .await?,
        SessionAnswer::Refused(SessionRefusal::NotWriter {
            current: Some(epoch(2)?),
        }),
    )?;
    let address = ObjectAddress::Copy(fixture.project.clone());
    let transfer = TransferId::new(uuid::Uuid::now_v7().to_string())?;
    equal(
        client
            .call(
                address.clone(),
                WireCall::Copy(CopyCall::Fence {
                    transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
                    new_epoch: epoch(4)?,
                    holder: b.installation.clone(),
                    expect: FenceExpectation {
                        epoch: epoch(2)?,
                        acked: position(1, 3)?,
                    },
                    target_schema_max: 10,
                }),
            )
            .await?,
        WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
            old_epoch: epoch(2)?,
            new_epoch: epoch(4)?,
            start: position(1, 3)?,
        })),
    )?;
    equal(
        client
            .call(
                address.clone(),
                WireCall::Copy(CopyCall::Fence {
                    transfer_id: transfer.clone(),
                    new_epoch: epoch(3)?,
                    holder: b.installation.clone(),
                    expect: FenceExpectation {
                        epoch: epoch(2)?,
                        acked: position(1, 3)?,
                    },
                    target_schema_max: 10,
                }),
            )
            .await?,
        WireResult::Copy(CopyResult::Fence(FenceResult::FenceNotAdvanced {
            current_epoch: epoch(4)?,
        })),
    )?;
    equal(
        client
            .call(
                address,
                WireCall::Copy(CopyCall::Fence {
                    transfer_id: transfer,
                    new_epoch: epoch(5)?,
                    holder: b.installation.clone(),
                    expect: FenceExpectation {
                        epoch: epoch(4)?,
                        acked: position(1, 3)?,
                    },
                    target_schema_max: 10,
                }),
            )
            .await?,
        WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
            old_epoch: epoch(4)?,
            new_epoch: epoch(5)?,
            start: position(1, 3)?,
        })),
    )?;
    a_socket.close().await?;
    b_socket.close().await?;
    Ok(())
}
async fn authority_and_existing_copy(client: &ConformanceClient) -> ConformanceResult<()> {
    let fixture = ImportedProjectFixture::new()?;
    let machine = HolderFixture::enroll(client, "import-denied").await?;
    let status = client
        .client
        .post(format!(
            "{}/admin/v1/import/projects/{}",
            client.base_url,
            fixture.project.as_str()
        ))
        .header(
            "Cf-Access-Jwt-Assertion",
            crate::machine_assertion(&machine.token)?,
        )
        .bearer_auth(&machine.token)
        .json(fixture.pages.first().ok_or("first")?)
        .send()
        .await?
        .status();
    equal(status, reqwest::StatusCode::UNAUTHORIZED)?;
    let held = crate::session_scenarios::SessionFixture::create(client).await?;
    let before = held.copy().await?;
    let mut offered = fixture.pages.first().ok_or("first")?.clone();
    offered.page.project = held.project.clone();
    equal(
        client.import_project(&held.project, &offered).await?,
        ProjectImportAnswer::ImportConflict {
            existing: ExistingImport::NotImported,
            offered: offered.digest,
        },
    )?;
    equal(held.copy().await?, before)?;
    held.close().await
}
pub(crate) async fn run_import_validation(client: &ConformanceClient) -> ConformanceResult<()> {
    staging(client).await?;
    schema_and_restore(client).await?;
    authority_and_existing_copy(client).await
}
