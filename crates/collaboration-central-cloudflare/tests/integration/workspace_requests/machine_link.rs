//! One original socket across real object eviction, live authority and SQLite effects.
mod alarm_rearm;
mod batch;
mod delivery;
mod fixture;
mod probe_diagnostic;
mod receipt;
mod renewal;
mod request_status;
mod witness_assertions;

use super::workerd_host::c2_controls::{
    AttachmentSnapshot, RestoreOutcome, WitnessPhase, WitnessRecord,
};
use central_link_protocol::{CorrelationId, DataFrame, ReplyPayload};
use central_storage::{SqlValue, Statement};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use fixture::{ConnectedLink, Exchange, Fixture, INSTALLATION, ORGANIZATION, OTHER_ORGANIZATION};
use futures_util::{FutureExt, SinkExt, StreamExt};
use receipt::{AreaReceipt, HibernationReceipt};
use reqwest_websocket::{CloseCode, Message, RequestBuilderExt};
use runtime_primitives::Clock;
use std::{io::Write, panic::AssertUnwindSafe, path::PathBuf, time::Duration};
use workspace_model::{
    Caller, ItemRef, ItemView, RefusalReason, WorkspaceAnswer, WorkspaceId, WorkspaceOperation,
    WorkspaceReply, WorkspaceRequest, WorkspaceResult,
};

const WORKSPACE_NAME: &str = "Real hibernation workspace";
const AREA_NAME: &str = "Area created after actual hibernation";

#[tokio::test]
async fn worker_keeps_the_link_open_after_an_unknown_ack() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = async {
        let mut link = ConnectedLink::open(&fixture).await?;
        fixture::send(
            &mut link.socket,
            &DataFrame::Ack {
                message_id: "019f0000-0000-7000-8000-00000000cf39".parse()?,
                answer: central_link_protocol::DeliveryAnswer::Accepted,
            },
        )
        .await?;
        let exchange = link
            .call(WorkspaceRequest {
                request_id: None,
                workspace_id: None,
                operation: WorkspaceOperation::List {
                    query: workspace_model::ListQuery::Workspaces {
                        include_archived: false,
                    },
                    cursor: workspace_model::ListCursor::start(
                        workspace_model::ListOrder::OldestFirst,
                    ),
                    limit: 10.try_into()?,
                },
            })
            .await?;
        if !matches!(
            workspace_reply(&exchange.reply)?.result,
            WorkspaceResult::Done(_)
        ) {
            return Err("the same socket did not answer a workspace read after the Ack".into());
        }
        link.close().await
    }
    .await;
    fixture.close().await?;
    result
}

#[tokio::test]
async fn hibernating_socket_restores_live_authority_and_semantic_store() -> Result<()> {
    let destination = std::env::var_os("CODEX_ROUTER_C2_HIBERNATION_RECEIPT").map(PathBuf::from);
    if destination.as_ref().is_some_and(|path| !path.is_absolute()) {
        return Err("hibernation receipt destination must be absolute".into());
    }
    let fixture = Fixture::new().await?;
    // Even a failed journey reaches awaited runtime disposal before returning its error.
    let result = hibernation_journey(&fixture).await;
    let probe_enabled =
        std::env::var("CODEX_ROUTER_C2_ORDINARY_ACCEPT_PROBE").as_deref() == Ok("1");
    let diagnostic = probe_diagnostic::collect(
        probe_enabled,
        result.is_err(),
        &fixture.host.captured_logs(),
    );
    let disposed = matches!(
        AssertUnwindSafe(fixture.close()).catch_unwind().await,
        Ok(Ok(()))
    );
    if let Some(marker) = probe_diagnostic::result_marker(diagnostic, disposed)? {
        eprintln!("{marker}");
    }
    if probe_enabled && result.is_err() {
        return Err("ordinary accept probe journey failed".into());
    }
    let mut receipt = result?;
    receipt.disposed = true;
    if let Some(destination) = destination {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        output.write_all(&serde_json::to_vec_pretty(&receipt)?)?;
    }
    Ok(())
}

async fn hibernation_journey(fixture: &Fixture) -> Result<HibernationReceipt> {
    let (mut link, baseline, workspace, before_events, attachment) = baseline(fixture).await?;
    fixture.host.c2_witness(WitnessPhase::Evicting).await?;
    fixture.host.c2_evict(&fixture.organization).await?;

    // Only external recorder controls and the same socket occur in this interval.
    fixture.host.c2_witness(WitnessPhase::Heartbeat).await?;
    let mut heartbeat_events = Vec::new();
    for _ in 0..3 {
        link.socket.send(Message::Text("ping".into())).await?;
        match link.socket.next().await {
            Some(Ok(Message::Text(text))) if text == "pong" => {}
            _ => return Err("hibernated socket did not return the literal platform pong".into()),
        }
        let events = fixture.host.c2_witness(WitnessPhase::Heartbeat).await?;
        if events != before_events {
            return Err("heartbeat woke a constructor or handler".into());
        }
        heartbeat_events.push(events);
    }
    fixture.host.c2_witness(WitnessPhase::Deciding).await?;
    let deciding = link.call(create_area(&workspace)?).await?;
    // Capture before Show, raw store inspection, phase changes or socket close.
    let deciding_events = fixture.host.c2_witness(WitnessPhase::Deciding).await?;
    let (instance, restored_sequence) = witness_assertions::reconstruction(
        &before_events,
        &deciding_events,
        &attachment,
        correlation(&deciding.request)?,
        RestoreOutcome::Admitted,
    )?;
    witness_assertions::alarm_snapshot(
        &deciding_events,
        correlation(&deciding.request)?,
        instance,
        restored_sequence,
    )?;
    let WorkspaceResult::Done(WorkspaceAnswer::Created {
        item: ItemRef::Area(area_id),
        ..
    }) = &workspace_reply(&deciding.reply)?.result
    else {
        return Err("deciding CreateArea did not return an actual created area".into());
    };
    let area_id = area_id.clone();

    fixture.host.c2_witness(WitnessPhase::Settling).await?;
    let read = link
        .call(WorkspaceRequest {
            request_id: None,
            workspace_id: Some(workspace.clone()),
            operation: WorkspaceOperation::Show {
                item: ItemRef::Area(area_id.clone()),
            },
        })
        .await?;
    match &workspace_reply(&read.reply)?.result {
        WorkspaceResult::Done(WorkspaceAnswer::Item {
            item: ItemView::Area(area),
        }) if area.area_id == area_id
            && area.workspace_id == workspace
            && area.name.as_str() == AREA_NAME => {}
        _ => return Err("real Show did not return the created area ID and literal name".into()),
    }
    let store = fixture
        .runtime()
        .inspect_workspace(
            &workspace,
            Statement {
                sql: "SELECT area_id,name FROM workspace_areas ORDER BY area_id",
                args: vec![],
            },
        )
        .await?;
    if store.columns != vec!["area_id", "name"]
        || store.rows
            != vec![vec![
                SqlValue::Text(area_id.to_string()),
                SqlValue::Text(AREA_NAME.into()),
            ]]
    {
        return Err("owning workspace store differs from the actual created area".into());
    }
    let welcome = link.welcome.clone();
    link.close().await?;
    Ok(HibernationReceipt {
        kind: "c2Hibernation",
        organization: fixture.organization.clone(),
        machine_id: INSTALLATION.parse()?,
        welcome,
        baseline_request: baseline.request,
        baseline_reply: baseline.reply,
        deciding_request: deciding.request,
        deciding_reply: deciding.reply,
        read_request: read.request,
        read_reply: read.reply,
        before_events,
        heartbeat_events,
        deciding_events,
        area: AreaReceipt {
            area_id,
            name: AREA_NAME,
        },
        store,
        disposed: false,
    })
}

async fn baseline(
    fixture: &Fixture,
) -> Result<(
    ConnectedLink,
    Exchange,
    WorkspaceId,
    Vec<WitnessRecord>,
    AttachmentSnapshot,
)> {
    fixture.host.c2_witness(WitnessPhase::Connecting).await?;
    let mut link = ConnectedLink::open(fixture).await?;
    match &link.welcome {
        DataFrame::Welcome {
            machine_id,
            central_time,
        } if machine_id.as_str() == INSTALLATION
            && central_time.timestamp_millis() == fixture.clock.now().unix_millis() => {}
        _ => return Err("Welcome must expose the machine_id and one injected clock".into()),
    }
    fixture.host.c2_witness(WitnessPhase::Baseline).await?;
    let exchange = link
        .call(WorkspaceRequest {
            request_id: Some("019f0000-0000-7000-8000-00000000cf10".parse()?),
            workspace_id: None,
            operation: WorkspaceOperation::CreateWorkspace {
                name: WORKSPACE_NAME.to_owned().try_into()?,
            },
        })
        .await?;
    let WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { workspace }) =
        &workspace_reply(&exchange.reply)?.result
    else {
        return Err("actual baseline CreateWorkspace failed".into());
    };
    if workspace.name.as_str() != WORKSPACE_NAME {
        return Err("baseline workspace name changed".into());
    }
    let workspace = workspace.workspace_id.clone();
    fixture.allow_workspace(&workspace).await?;
    let events = fixture.host.c2_witness(WitnessPhase::Baseline).await?;
    let attachment =
        witness_assertions::baseline_attachment(&events, correlation(&exchange.request)?)?;
    Ok((link, exchange, workspace, events, attachment))
}

fn create_area(workspace: &WorkspaceId) -> Result<WorkspaceRequest> {
    Ok(WorkspaceRequest {
        request_id: Some("019f0000-0000-7000-8000-00000000cf11".parse()?),
        workspace_id: Some(workspace.clone()),
        operation: WorkspaceOperation::CreateArea {
            name: AREA_NAME.to_owned().try_into()?,
            description: "Test description".to_owned().try_into()?,
        },
    })
}

fn correlation(frame: &DataFrame) -> Result<&CorrelationId> {
    match frame {
        DataFrame::Request { request } => Ok(request.correlation_id()),
        _ => Err("expected an actual Request frame".into()),
    }
}

fn workspace_reply(frame: &DataFrame) -> Result<&WorkspaceReply> {
    match frame {
        DataFrame::Reply { result, .. } => match result.as_ref() {
            ReplyPayload::Workspace(reply) => Ok(reply),
            ReplyPayload::Mail(_) => Err("Workspace request returned a Mail reply".into()),
        },
        _ => Err("expected an actual Reply frame, with no second Welcome".into()),
    }
}

#[tokio::test]
async fn worker_link_requires_header_auth_matching_organization_and_public_caller() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = route_and_caller_refusals(&fixture).await;
    fixture.close().await?;
    result
}

async fn route_and_caller_refusals(fixture: &Fixture) -> Result<()> {
    let plain = fixture
        .client
        .get(fixture.link_url(ORGANIZATION))
        .bearer_auth(&fixture.token)
        .send()
        .await?;
    if plain.status().as_u16() != 426 {
        return Err("plain GET must refuse WebSocket admission".into());
    }
    let non_v7 = signed::machine_token(
        ORGANIZATION,
        "019f0000-0000-4000-8000-00000000cf02",
        fixture.enrollment.as_str(),
        signed::NOW_SECONDS,
        signed::NOW_SECONDS + 600,
    )?;
    // A well-formed machine_id with no machine row is refused.
    let unknown_installation = signed::machine_token(
        ORGANIZATION,
        "019f0000-0000-7000-8000-00000000cf0f",
        fixture.enrollment.as_str(),
        signed::NOW_SECONDS,
        signed::NOW_SECONDS + 600,
    )?;
    for request in [
        fixture.client.get(fixture.link_url(ORGANIZATION)),
        fixture.client.get(format!(
            "{}?token={}",
            fixture.link_url(ORGANIZATION),
            fixture.token
        )),
        fixture
            .client
            .get(fixture.link_url(ORGANIZATION))
            .bearer_auth(signed::tamper_signature(&fixture.token)?),
        fixture
            .client
            .get(fixture.link_url(ORGANIZATION))
            .bearer_auth(non_v7),
        fixture
            .client
            .get(fixture.link_url(ORGANIZATION))
            .bearer_auth(unknown_installation),
        fixture
            .client
            .get(fixture.link_url(OTHER_ORGANIZATION))
            .bearer_auth(&fixture.token),
    ] {
        // A transport error must not print the query-carried fixture JWT.
        let response = request
            .upgrade()
            .send()
            .await
            .map_err(|_| "authentication refusal transport failed")?;
        if response.status().as_u16() != 401 {
            return Err("invalid upgrade must refuse before Welcome".into());
        }
    }
    let mut link = ConnectedLink::open(fixture).await?;
    let exchange = link
        .call_as(
            Caller::Person {
                machine_id: "019f0000-0000-7000-8000-00000000cf20".parse()?,
                person_id: signed::OWNER.parse()?,
            },
            WorkspaceRequest {
                request_id: Some("019f0000-0000-7000-8000-00000000cf21".parse()?),
                workspace_id: None,
                operation: WorkspaceOperation::CreateWorkspace {
                    name: "Must not create".to_owned().try_into()?,
                },
            },
        )
        .await?;
    if workspace_reply(&exchange.reply)?.result
        != WorkspaceResult::Refused(RefusalReason::MachineRevoked)
    {
        return Err("caller mismatch must produce the typed MachineRevoked refusal".into());
    }
    if !fixture
        .runtime()
        .read(Statement {
            sql: "SELECT workspace_id FROM directory_workspaces",
            args: vec![],
        })
        .await?
        .rows
        .is_empty()
    {
        return Err("refused caller created a workspace".into());
    }
    link.close().await
}

enum AuthorityChange {
    Expiry,
    Revocation,
    Supersession,
}

#[tokio::test]
async fn hibernating_socket_refuses_expired_original_grant() -> Result<()> {
    authority_refusal(AuthorityChange::Expiry).await
}

#[tokio::test]
async fn hibernating_socket_refuses_revoked_enrollment() -> Result<()> {
    authority_refusal(AuthorityChange::Revocation).await
}

#[tokio::test]
async fn hibernating_socket_refuses_superseded_enrollment() -> Result<()> {
    authority_refusal(AuthorityChange::Supersession).await
}

async fn authority_refusal(change: AuthorityChange) -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = denied_journey(&fixture, change).await;
    fixture.close().await?;
    result
}

async fn denied_journey(fixture: &Fixture, change: AuthorityChange) -> Result<()> {
    let (mut link, _, workspace, _, attachment) = baseline(fixture).await?;
    match change {
        AuthorityChange::Expiry => {}
        AuthorityChange::Revocation => {
            let ended: std::result::Result<
                Option<collaboration_central_core::identity_contract::EndedMachineClient>,
                central_storage::StoreFailure,
            > = fixture
                .runtime()
                .control(
                    "machine-login-revoke",
                    &collaboration_central_core::MachineLoginControl::Revoke {
                        person_id: signed::OWNER.to_owned().try_into()?,
                        operation_id: "019f0000-0000-7000-8000-00000000cf30"
                            .to_owned()
                            .try_into()?,
                        machine_id: INSTALLATION.to_owned().try_into()?,
                        enrollment_id: fixture.enrollment.clone(),
                    },
                )
                .await?;
            if !matches!(ended?,Some(ended) if ended.enrollment_id == fixture.enrollment) {
                return Err("real revocation did not end the original client".into());
            }
        }
        AuthorityChange::Supersession => fixture.supersede().await?,
    }
    let before = fixture.host.c2_witness(WitnessPhase::Evicting).await?;
    fixture.host.c2_evict(&fixture.organization).await?;
    if matches!(change, AuthorityChange::Expiry) {
        fixture.clock.advance(Duration::from_secs(600))?;
        fixture
            .host
            .set_external_time_ms(fixture.clock.now().unix_millis())
            .await?;
    }
    fixture.host.c2_witness(WitnessPhase::Deciding).await?;
    let request = link.request_as(fixture::session_caller()?, create_area(&workspace)?)?;
    fixture::send(&mut link.socket, &request).await?;
    // Both hosts answer the request MachineRevoked under its correlation, then
    // close: the Router's only way on is a fresh connection.
    let reply = fixture::receive(&mut link.socket).await?;
    let refused = matches!(
        &reply,
        DataFrame::Reply { correlation_id, .. } if correlation_id == correlation(&request)?
    ) && workspace_reply(&reply)?.result
        == WorkspaceResult::Refused(RefusalReason::MachineRevoked);
    if !refused {
        return Err(format!("stale authority must answer MachineRevoked first: {reply:?}").into());
    }
    match link.socket.next().await {
        Some(Ok(Message::Close {
            code: CloseCode::Policy,
            reason,
        })) if reason == "machine authority expired or revoked" => {}
        other => {
            return Err(format!(
                "stale authority must close the original socket before a semantic effect: {other:?}"
            )
            .into());
        }
    }
    let deciding = fixture.host.c2_witness(WitnessPhase::Deciding).await?;
    witness_assertions::reconstruction(
        &before,
        &deciding,
        &attachment,
        correlation(&request)?,
        RestoreOutcome::Refused,
    )?;
    fixture.host.c2_witness(WitnessPhase::Settling).await?;
    let store = fixture
        .runtime()
        .inspect_workspace(
            &workspace,
            Statement {
                sql: "SELECT area_id,name FROM workspace_areas",
                args: vec![],
            },
        )
        .await?;
    if !store.rows.is_empty() {
        return Err("stale authority changed the owning workspace store".into());
    }
    // The server's actual close frame is the terminal socket result.
    drop(link);
    Ok(())
}
