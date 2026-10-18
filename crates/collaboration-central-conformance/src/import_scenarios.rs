use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, holder_fixtures::*, import_fixtures::*,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
pub async fn run_import_claim_roundtrip(client: &ConformanceClient) -> ConformanceResult<()> {
    client.set_time(time()?).await?;
    let fixture = ImportedProjectFixture::new()?;
    crate::fleet_directory_fixture::import_identity(client, &fixture.project).await?;
    fixture.upload(client).await?;
    equal(fixture.state(client).await?.holder, None)?;
    let updates = directory_updates(client).await?;
    check(updates.iter().any(|update|matches!(update,DirectoryUpdate::Summary{project,summary,position:head,..} if project==&fixture.project && summary==&fixture.summary && head.commit_id.value()==3)),"import producer publishes summary before claim")?;
    check(updates.iter().any(|update|matches!(update,DirectoryUpdate::Lease{project,lease:DirectoryLease::Unheld,..} if project==&fixture.project)),"registry producer publishes Unheld lease side")?;
    let a = HolderFixture::enroll(client, "claim-A").await?;
    let b = HolderFixture::enroll(client, "claim-B").await?;
    let (mut a_socket, _) = a.connect(client).await?;
    let (mut b_socket, _) = b.connect(client).await?;
    let read = a_socket
        .request(SessionRequest::Read {
            scope: ReadScope::ProjectCopy {
                project: fixture.project.clone(),
            },
            after: None,
        })
        .await?;
    let expected_rows = fixture
        .pages
        .iter()
        .flat_map(|page| page.page.rows.clone())
        .collect::<Vec<_>>();
    check(
        matches!(read,SessionAnswer::ReadPage(ReadPage::ProjectCopy(ProjectCopyRead::Page(page))) if page.position==position(1,3)? && page.rows==expected_rows),
        "imported unheld copy readable through native machine session",
    )?;
    equal(
        b_socket
            .request(SessionRequest::Forward {
                project: fixture.project.clone(),
                post: ForwardedPost {
                    kind: ForwardedPostKind::ThreadPost,
                    body: BoardPayload::new(serde_json::json!({"body":"unheld"})),
                },
            })
            .await?,
        SessionAnswer::Forwarded(ForwardOutcome::NotHeld),
    )?;
    equal(
        a_socket
            .request(SessionRequest::Piece(piece(
                &fixture.project,
                position(1, 3)?,
                position(1, 4)?,
                vec![rows(4, vec![row(4, "unheld-refused")])],
            )?))
            .await?,
        SessionAnswer::Refused(SessionRefusal::NotHeld),
    )?;
    let claim_attempt = attempt()?;
    client.control(ControlRequest::HoldNextFence).await?;
    let a_frame = send_request(
        &mut a_socket,
        claim_request(&fixture.project, claim_attempt.clone()),
    )
    .await?;
    client.control(ControlRequest::AwaitHeldFence).await?;
    equal(fixture.state(client).await?.fence, epoch(1)?)?;
    equal(
        b_socket
            .request(claim_request(&fixture.project, attempt()?))
            .await?,
        SessionAnswer::Refused(SessionRefusal::AlreadyHeld {
            holder: a.installation.clone(),
        }),
    )?;
    // A same-socket read cannot see a Claim answer before its copy is fenced.
    let _ = a_socket
        .request(SessionRequest::Read {
            scope: ReadScope::ProjectCopy {
                project: fixture.project.clone(),
            },
            after: None,
        })
        .await?;
    client.control(ControlRequest::ResumeFence).await?;
    let granted = receive_answer(&mut a_socket, &a_frame).await?;
    equal(
        granted.clone(),
        SessionAnswer::Claimed {
            epoch: epoch(2)?,
            expires_at: time()?.0 + chrono::TimeDelta::hours(4),
            start: position(1, 3)?,
        },
    )?;
    equal(
        fixture.state(client).await?.holder,
        Some(a.installation.clone()),
    )?;
    equal(
        a_socket
            .request(claim_request(&fixture.project, claim_attempt))
            .await?,
        granted,
    )?;
    equal(
        a_socket
            .request(SessionRequest::Piece(piece(
                &fixture.project,
                position(1, 3)?,
                position(2, 1)?,
                vec![rows(1, vec![row(4, "after-claim")])],
            )?))
            .await?,
        SessionAnswer::Acked {
            through: position(2, 1)?,
        },
    )?;
    equal(
        b_socket
            .request(claim_request(&fixture.project, attempt()?))
            .await?,
        SessionAnswer::Refused(SessionRefusal::AlreadyHeld {
            holder: a.installation.clone(),
        }),
    )?;
    equal(
        b_socket
            .request(claim_request(&project()?, attempt()?))
            .await?,
        SessionAnswer::Refused(SessionRefusal::NotFound),
    )?;
    let read = b_socket
        .request(SessionRequest::Read {
            scope: ReadScope::ProjectCopy {
                project: fixture.project.clone(),
            },
            after: None,
        })
        .await?;
    check(
        matches!(read,SessionAnswer::ReadPage(ReadPage::ProjectCopy(ProjectCopyRead::Page(page))) if page.position==position(2,1)? && page.rows.last()==Some(&vec![SqlValue::Integer(4),SqlValue::Text("after-claim".into())]) && page.rows.len()==4),
        "first claimed piece is stored at the imported head",
    )?;
    let before = directory_updates(client).await?;
    let first = fixture.pages.first().ok_or("first page")?;
    equal(
        client.import_project(&fixture.project, first).await?,
        ProjectImportAnswer::AlreadyImported,
    )?;
    equal(directory_updates(client).await?, before)?;
    let mut offered = first.clone();
    offered.digest = ImportDigest::new("e".repeat(64))?;
    equal(
        client.import_project(&fixture.project, &offered).await?,
        ProjectImportAnswer::ImportConflict {
            existing: ExistingImport::Imported(fixture.digest()?),
            offered: offered.digest,
        },
    )?;
    equal(fixture.state(client).await?.acked, position(2, 1)?)?;
    let post = ForwardedPost {
        kind: ForwardedPostKind::ThreadPost,
        body: BoardPayload::new(
            serde_json::json!({"request_id":"M4-holder-admission-stand-in","body":"post after import claim"}),
        ),
    };
    let forwarded_request = send_request(
        &mut b_socket,
        SessionRequest::Forward {
            project: fixture.project.clone(),
            post: post.clone(),
        },
    )
    .await?;
    let forward_id = loop {
        match a_socket.receive().await? {
            CentralFrame::Notice { .. } => {}
            CentralFrame::Forwarded {
                forward_id,
                origin,
                origin_label,
                project,
                post: actual,
            } => {
                equal(origin, b.installation.clone())?;
                equal(origin_label.as_str(), "claim-B")?;
                equal(project, fixture.project.clone())?;
                equal(actual, post)?;
                break forward_id;
            }
            other => return Err(format!("forward after claim {other:?}").into()),
        }
    };
    let reply = BoardPayload::new(
        serde_json::json!({"fixture":"M4-holder-admission-stand-in","accepted":true}),
    );
    a_socket
        .send(&MachineFrame::ForwardAnswer {
            forward_id,
            reply: reply.clone(),
        })
        .await?;
    equal(
        receive_answer(&mut b_socket, &forwarded_request).await?,
        SessionAnswer::Forwarded(ForwardOutcome::Answered { reply }),
    )?;
    a_socket.close().await?;
    b_socket.close().await?;
    let (socket, welcome) = a.connect(client).await?;
    let expected_position = position(2, 1)?;
    check(
        welcome.iter().any(|state| {
            state.project == fixture.project
                && state.acked == expected_position
                && state.lease.epoch.value() == 2
        }),
        "Welcome includes claimed project and acknowledged prefix",
    )?;
    socket.close().await?;
    println!(
        "IMPORT real clients: owner import -> unheld copy read -> A Fencing held -> B overlapping claim AlreadyHeld -> A epoch 2, first piece at imported head -> B post delivered to A and answered; M4 holder admission is a fixed stand-in"
    );
    Ok(())
}
pub async fn run_import_claim_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    run_import_claim_roundtrip(client).await?;
    crate::import_validation::run_import_validation(client).await?;
    println!(
        "IMPORT staging/replay/order/digest/schema/full rollback, schema support, restore guard, owner edge and held-copy conflicts: passed; directory projection has its dedicated F1/F2 journey"
    );
    Ok(())
}
