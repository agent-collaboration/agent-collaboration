use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, HolderSession,
    fleet_directory_fixture::import_identity, holder_fixtures::*,
    import_fixtures::ImportedProjectFixture,
};
use collaboration_central_core::{DirectoryUpdate, RegistryTime};
use collaboration_replication_protocol::*;
use fleet_admission::{
    CopyMetadata, CopyMetadataAsOf, DirectoryEntry, DirectoryQuery, DirectorySide, LeaseMetadata,
};

pub struct ImportDirectoryRestartScenario {
    fixture: ImportedProjectFixture,
    reader: HolderFixture,
    identity: DirectoryEntry,
    at: RegistryTime,
}
impl ImportDirectoryRestartScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let future = chrono::Utc::now() + chrono::TimeDelta::minutes(2);
        let at = RegistryTime(
            chrono::DateTime::from_timestamp_millis(future.timestamp_millis())
                .ok_or("directory fixture clock")?,
        );
        client.set_time(at).await?;
        let reader = HolderFixture::enroll(client, "directory-reader").await?;
        let (mut socket, welcome) = reader.connect(client).await?;
        check(welcome.is_empty(), "directory reader owns no project")?;

        let normal = ImportedProjectFixture::new()?;
        let identity = import_identity(client, &normal.project).await?;
        equal(read_entry(&mut socket, &identity).await?, identity.clone())?;
        check(
            read_query(
                &mut socket,
                DirectoryQuery {
                    workspace: Some(identity.workspace.clone()),
                    status: Some("imported".into()),
                    ..DirectoryQuery::default()
                },
            )
            .await?
            .is_empty(),
            "NotYetReported does not match status filter",
        )?;
        normal.upload(client).await?;
        let published = imported_entry(identity, &normal, at)?;
        equal(normal.state(client).await?.holder, None)?;
        assert_published_copy(&mut socket, &normal).await?;
        equal(
            read_entry(&mut socket, &published).await?,
            published.clone(),
        )?;
        equal(
            read_query(
                &mut socket,
                DirectoryQuery {
                    text: Some("DIRECTORY PROJECT".into()),
                    workspace: Some(published.workspace.clone()),
                    board: Some(published.board.clone()),
                    status: Some("imported".into()),
                    holder: None,
                },
            )
            .await?,
            vec![published.clone()],
        )?;
        check(
            read_query(
                &mut socket,
                DirectoryQuery {
                    workspace: Some(published.workspace.clone()),
                    holder: Some(reader.installation.clone()),
                    ..DirectoryQuery::default()
                },
            )
            .await?
            .is_empty(),
            "Known(Unheld) has no matching holder",
        )?;
        println!(
            "DIRECTORY F1: actual public entry has exact identity, status/latest-record/position/acked-at and Known(Unheld)/registry-as-of before any claim; missing sides and filters passed"
        );

        let fixture = ImportedProjectFixture::new()?;
        let identity = import_identity(client, &fixture.project).await?;
        equal(read_entry(&mut socket, &identity).await?, identity.clone())?;
        client
            .control(ControlRequest::DropNextDirectoryUpdate)
            .await?;
        fixture.upload(client).await?;
        equal(fixture.state(client).await?.acked, position(1, 3)?)?;
        assert_published_copy(&mut socket, &fixture).await?;
        let mut missing_summary = identity.clone();
        missing_summary.lease_side = DirectorySide::Known(LeaseMetadata {
            lease: DirectoryLease::Unheld,
            as_of: at.0,
        });
        equal(read_entry(&mut socket, &identity).await?, missing_summary)?;
        println!(
            "DIRECTORY F2 precondition: exact committed copy rows/head readable; actual public copy side NotYetReported and lease Known(Unheld) after lost summary dispatch"
        );
        socket.close().await?;
        Ok(Self {
            fixture,
            reader,
            identity,
            at,
        })
    }
    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        client
            .set_time(RegistryTime(self.at.0 + chrono::TimeDelta::seconds(6)))
            .await?;
        let (mut socket, welcome) = self.reader.connect(client).await?;
        check(welcome.is_empty(), "restart reader remains a non-holder")?;
        let expected = imported_entry(self.identity.clone(), &self.fixture, self.at)?;
        equal(
            read_entry(&mut socket, &self.identity).await?,
            expected.clone(),
        )?;
        assert_published_copy(&mut socket, &self.fixture).await?;
        equal(summary_deliveries(client, &self.fixture.project).await?, 1)?;
        client
            .set_time(RegistryTime(self.at.0 + chrono::TimeDelta::seconds(12)))
            .await?;
        equal(read_entry(&mut socket, &self.identity).await?, expected)?;
        println!("DIRECTORY F2 second alarm: public entry equality retained");
        equal(summary_deliveries(client, &self.fixture.project).await?, 1)?;
        socket.close().await?;
        println!(
            "DIRECTORY F2 restart: alarm repaired actual public Known(summary/position/source-acked-at), retained independent Known(Unheld)/registry-as-of; next alarm preserves exact entry and sends no repeat summary, no reimport or claim"
        );
        Ok(())
    }
}
async fn summary_deliveries(
    client: &ConformanceClient,
    project: &ProjectId,
) -> ConformanceResult<usize> {
    Ok(crate::import_fixtures::directory_updates(client).await?.iter().filter(|update| {
        matches!(update, DirectoryUpdate::Summary { project: received, .. } if received == project)
    }).count())
}
fn imported_entry(
    mut identity: DirectoryEntry,
    fixture: &ImportedProjectFixture,
    at: RegistryTime,
) -> ConformanceResult<DirectoryEntry> {
    identity.copy_side = DirectorySide::Known(CopyMetadata {
        status: "imported".into(),
        latest_record_at: time()?.0,
        as_of: CopyMetadataAsOf {
            position: position(1, 3)?,
            acked_at: at.0,
        },
    });
    equal(&fixture.summary.status, &"imported".to_owned())?;
    equal(fixture.summary.latest_record_at, time()?.0)?;
    identity.lease_side = DirectorySide::Known(LeaseMetadata {
        lease: DirectoryLease::Unheld,
        as_of: at.0,
    });
    Ok(identity)
}
async fn read_query(
    socket: &mut HolderSession,
    query: DirectoryQuery,
) -> ConformanceResult<Vec<DirectoryEntry>> {
    let answer = socket
        .request(SessionRequest::Read {
            scope: ReadScope::Directory {
                query: FleetPayload::new(serde_json::to_value(query)?),
            },
            after: None,
        })
        .await?;
    let SessionAnswer::ReadPage(ReadPage::Directory { entries, .. }) = answer else {
        return Err(format!("expected actual directory read, got {answer:?}").into());
    };
    entries
        .into_iter()
        .map(|entry| serde_json::from_value(entry.into_value()).map_err(Into::into))
        .collect()
}
pub(crate) async fn read_entry(
    socket: &mut HolderSession,
    identity: &DirectoryEntry,
) -> ConformanceResult<DirectoryEntry> {
    let entries = read_query(
        socket,
        DirectoryQuery {
            workspace: Some(identity.workspace.clone()),
            board: Some(identity.board.clone()),
            ..DirectoryQuery::default()
        },
    )
    .await?;
    let [entry] = entries.as_slice() else {
        return Err(format!("expected one actual Fleet directory entry, got {entries:?}").into());
    };
    equal(&entry.project, &identity.project)?;
    Ok(entry.clone())
}
async fn assert_published_copy(
    socket: &mut HolderSession,
    fixture: &ImportedProjectFixture,
) -> ConformanceResult<()> {
    let answer = socket
        .request(SessionRequest::Read {
            scope: ReadScope::ProjectCopy {
                project: fixture.project.clone(),
            },
            after: None,
        })
        .await?;
    let SessionAnswer::ReadPage(ReadPage::ProjectCopy(ProjectCopyRead::Page(page))) = answer else {
        return Err("expected published copy page".into());
    };
    equal(page.project, fixture.project.clone())?;
    equal(page.position, position(1, 3)?)?;
    equal(page.table, "records".to_owned())?;
    equal(page.next, None)?;
    equal(
        page.rows,
        vec![
            vec![SqlValue::Integer(1), SqlValue::Text("imported-1".into())],
            vec![SqlValue::Integer(2), SqlValue::Text("imported-2".into())],
            vec![SqlValue::Integer(3), SqlValue::Text("imported-3".into())],
        ],
    )?;
    println!("DIRECTORY committed copy control: public exact rows1/2/3 at(1,3), one complete page");
    Ok(())
}
