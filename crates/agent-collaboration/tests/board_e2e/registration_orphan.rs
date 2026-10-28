//! Controlled native registration refusal after a real accepted directory creation.
#![deny(clippy::too_many_lines)]
use super::{
    fixture::{BoardFixture, at, fields, header, identity},
    owned_host::StoppedHost,
};
use board_central_test_support::{DispatchFaultObservation, ObjectAddress, RuntimeControl};
use codex_router_test_support::{FixtureResult, scenario_finalization::run_scenario};
use message_board::{ProjectId, RecordKind};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

fn connection(fixture: &BoardFixture) -> FixtureResult<ObjectAddress> {
    Ok(ObjectAddress::Connection(
        String::from(
            fixture
                .installation
                .as_ref()
                .ok_or("installation absent")?
                .service_id
                .clone(),
        )
        .try_into()?,
    ))
}

fn central(fixture: &BoardFixture) -> FixtureResult<&board_central_test_support::TestCentral> {
    fixture
        .central
        .as_ref()
        .ok_or_else(|| "central absent".into())
}

pub async fn directory_collision(fixture: &BoardFixture) -> FixtureResult {
    let project = identity();
    let path = fixture.directory.join("projects").join(&project);
    assert!(!path.exists());
    let request = fields(
        header(&fixture.owner),
        json!({"projectId":project,
        "boardId":fixture.board,"title":"Rust journey project"}),
    )?;
    let reply = fixture.host()?.mcp("project_create", request).await?;
    assert_eq!(
        reply.pointer("/result/kind"),
        Some(&json!("scopeUnavailable"))
    );
    assert_eq!(
        reply.pointer("/result/decision/kind"),
        Some(&json!("projectRejected"))
    );
    assert_eq!(
        reply.pointer("/result/decision/reason/kind"),
        Some(&json!("directory"))
    );
    assert!(
        !path.exists(),
        "directory rejection constructed a native writer"
    );
    Ok(())
}

pub async fn arrange(fixture: &mut BoardFixture) -> FixtureResult<Value> {
    let project = identity();
    let request = fields(
        header(&fixture.owner),
        json!({"projectId":project,
        "boardId":fixture.board,"title":format!("controlled registration refusal {project}")}),
    )?;
    let source = connection(fixture)?;
    run_scenario(
        (&mut *fixture, source),
        async |state: &mut (&mut BoardFixture, ObjectAddress)| {
            let (fixture, source) = state;
            let armed = central(fixture)?
                .runtime_control(
                    source.clone(),
                    RuntimeControl::HoldNextDispatch {
                        destination: ObjectAddress::Registry,
                    },
                )
                .await?;
            assert!(matches!(
                armed.dispatch,
                DispatchFaultObservation::Armed { .. }
            ));
            // Do not retry: the accepted directory and actual Register must remain pending.
            let reply = fixture
                .host()?
                .mcp("project_create", request.clone())
                .await?;
            assert_eq!(
                reply.pointer("/result/kind"),
                Some(&json!("projectCreationPending"))
            );
            hold_register(fixture, source.clone(), &project).await?;
            let stopped = fixture
                .host
                .take()
                .ok_or("pending Host absent")?
                .crash_for_readback()
                .await?;
            reject_native(fixture, &stopped, &project).await
        },
        async |(fixture, source): (&mut BoardFixture, ObjectAddress)| {
            let observed = central(fixture)?
                .runtime_control(source.clone(), RuntimeControl::Observe)
                .await?;
            if matches!(observed.dispatch, DispatchFaultObservation::Held { .. }) {
                central(fixture)?
                    .runtime_control(source, RuntimeControl::ResumeDispatch)
                    .await?;
            }
            Ok(())
        },
        || Ok(()),
    )
    .await?;
    fixture.reopen_host().await?;
    let reply = fixture
        .host()?
        .mcp("project_create", request.clone())
        .await?;
    assert_eq!(
        reply.pointer("/result/kind"),
        Some(&json!("scopeUnavailable"))
    );
    assert_eq!(
        reply.pointer("/result/decision/kind"),
        Some(&json!("projectRejected"))
    );
    assert_eq!(
        reply.pointer("/result/decision/reason/kind"),
        Some(&json!("registration"))
    );
    Ok(request)
}

async fn hold_register(
    fixture: &BoardFixture,
    source: ObjectAddress,
    project: &str,
) -> FixtureResult {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let observed = central(fixture)?
                .runtime_control(source.clone(), RuntimeControl::AwaitHeldDispatch)
                .await?;
            let DispatchFaultObservation::Held {
                destination,
                request,
            } = observed.dispatch
            else {
                return Err("Register did not reach actual dispatch hold".into());
            };
            assert_eq!(destination, ObjectAddress::Registry);
            let request: Value = serde_json::from_slice(&request)?;
            if request.get("kind") == Some(&json!("central.session-lease"))
                && request.pointer("/request/request/kind") == Some(&json!("register"))
                && request.pointer("/request/request/value/database") == Some(&json!(project))
            {
                return Ok(());
            }
            central(fixture)?
                .runtime_control(
                    source.clone(),
                    RuntimeControl::ResumeDispatchAndHoldNext {
                        destination: ObjectAddress::Registry,
                    },
                )
                .await?;
        }
    })
    .await?
}

async fn reject_native(
    fixture: &BoardFixture,
    stopped: &StoppedHost,
    project: &str,
) -> FixtureResult {
    if stopped.directory() != fixture.directory.canonicalize()? {
        return Err("stopped token belongs to another owned Host".into());
    }
    let project: ProjectId = project.to_owned().try_into()?;
    let peer = fixture
        .installation
        .as_ref()
        .ok_or("installation absent")?
        .peer_id
        .clone();
    let registry = collaboration_runtime::ProjectStoreRegistry::new(
        stopped.directory(),
        Some(peer.as_str().to_owned().try_into()?),
        Arc::new(runtime_primitives::SystemClock),
    );
    run_scenario(
        registry,
        async |registry: &mut collaboration_runtime::ProjectStoreRegistry| {
            let store = registry.open_project(&project).await?;
            let mut read = store.snapshot().await?;
            let projects = read.records(RecordKind::Project).await?;
            let requests = read.records(RecordKind::Request).await?;
            read.finish().await?;
            drop(store);
            assert!(
                projects.is_empty(),
                "pending registration already wrote its project"
            );
            assert!(
                requests.is_empty(),
                "pending registration already wrote a request ledger"
            );
            let database = registry.database(&project).await?;
            // The gate refusal is a controlled input. Normal Register policy remains separately tested.
            database
                .reject_registration(fleet_sync_protocol::GateReason {
                    code: fleet_sync_protocol::GateReasonCode::new("consumer.refused")?,
                    detail: "owned scope fixture registration refused".to_owned(),
                })
                .await?;
            drop(database);
            registry.orphan_project(&project).await?;
            Ok(())
        },
        async |registry: collaboration_runtime::ProjectStoreRegistry| {
            registry.close().await.map_err(Into::into)
        },
        || Ok(()),
    )
    .await
}

pub fn orphan_path(fixture: &BoardFixture, creation: &Value) -> FixtureResult<PathBuf> {
    let project = at(creation, "/projectId")?
        .as_str()
        .ok_or("orphan project absent")?;
    Ok(fixture
        .directory
        .join("projects")
        .join(project)
        .join("e1.orphaned.sqlite"))
}

pub async fn damage_after_stop(fixture: &BoardFixture, creation: &Value) -> FixtureResult {
    if fixture.host.is_some() {
        return Err("orphan damage requires joined Host".into());
    }
    let path = orphan_path(fixture, creation)?;
    if !path.is_file() {
        return Err("real moved-aside orphan file absent".into());
    }
    let project: ProjectId = at(creation, "/projectId")?
        .as_str()
        .ok_or("project absent")?
        .to_owned()
        .try_into()?;
    // No live writer exists; the malformed file is the explicit storage-failure input.
    std::fs::write(&path, [b'X'; 8192])?;
    match message_board_storage::ProjectLedgerReader::open(
        &path,
        project,
        board_fleet_sync::BoardProjectSchema::new()?,
    )
    .await
    {
        Ok(reader) => {
            reader.close().await;
            Err("damaged orphan still opened as a valid native ledger".into())
        }
        Err(_) => Ok(()),
    }
}
