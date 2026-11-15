//! Count successful clock responses while real dispatches persist the injected time.
use super::super::support::{self, Fixture, id, text};
use super::*;
use std::sync::atomic::Ordering;
use workspace_model as m;
use workspace_model::central_mail::{CentralMailAnswer, CentralMailOperation};

#[tokio::test]
async fn dispatch_and_reset_apply_one_clock_update_and_preserve_stored_time() {
    let fixture = Fixture::new().await;
    let mut observed = Vec::new();
    let runtime = fixture.host.cold_runtime(&fixture.caller.organization_id);
    let mut before = fixture
        .host
        .transport
        .clock_responses
        .load(Ordering::SeqCst);
    fixture.clock.0.store(1_861_920_000_100, Ordering::SeqCst);
    let area = fixture.area(7900).await;
    observed.push((
        "host workspace",
        fixture
            .host
            .transport
            .clock_responses
            .load(Ordering::SeqCst)
            - before,
    ));
    let stored = runtime
        .inspect_workspace(
            &fixture.workspace,
            Statement {
                sql: "SELECT created_at FROM areas WHERE area_id=?",
                args: vec![text(area)],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        stored.rows,
        vec![vec![text("2029-01-01T00:00:00.100+00:00")]]
    );

    before = fixture
        .host
        .transport
        .clock_responses
        .load(Ordering::SeqCst);
    fixture.clock.0.store(1_861_920_000_200, Ordering::SeqCst);
    let answer = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(7901),
                operation: CentralMailOperation::SendMessage {
                    to: support::session(&fixture.caller),
                    body: "clock dispatch".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap();
    observed.push((
        "host mail",
        fixture
            .host
            .transport
            .clock_responses
            .load(Ordering::SeqCst)
            - before,
    ));
    let CentralMailResult::Done(CentralMailAnswer::Queued { message_id }) = answer else {
        panic!("mail refused")
    };
    let stored = runtime
        .read(Statement {
            sql: "SELECT created_at FROM messages WHERE message_id=?",
            args: vec![text(message_id)],
        })
        .await
        .unwrap();
    assert_eq!(stored.rows, vec![vec![text("2029-01-01T00:00:00.200Z")]]);

    before = fixture
        .host
        .transport
        .clock_responses
        .load(Ordering::SeqCst);
    fixture.clock.0.store(1_861_920_000_300, Ordering::SeqCst);
    let answer = runtime
        .request(
            &fixture.caller,
            WorkspaceRequest {
                request_id: Some(id(7902)),
                workspace_id: Some(fixture.workspace.clone()),
                operation: m::WorkspaceOperation::CreateArea {
                    name: support::name("Direct clock"),
                },
            },
        )
        .await
        .unwrap();
    observed.push((
        "direct workspace",
        fixture
            .host
            .transport
            .clock_responses
            .load(Ordering::SeqCst)
            - before,
    ));
    let m::ItemRef::Area(area) = support::created(answer) else {
        panic!("area missing")
    };
    let stored = runtime
        .inspect_workspace(
            &fixture.workspace,
            Statement {
                sql: "SELECT created_at FROM areas WHERE area_id=?",
                args: vec![text(&area)],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        stored.rows,
        vec![vec![text("2029-01-01T00:00:00.300+00:00")]]
    );

    before = fixture
        .host
        .transport
        .clock_responses
        .load(Ordering::SeqCst);
    fixture.host.central(&fixture.caller.organization_id).await;
    observed.push((
        "explicit central",
        fixture
            .host
            .transport
            .clock_responses
            .load(Ordering::SeqCst)
            - before,
    ));
    before = fixture
        .host
        .transport
        .clock_responses
        .load(Ordering::SeqCst);
    fixture.clock.0.store(1_861_920_000_400, Ordering::SeqCst);
    fixture
        .host
        .reset_owners(&fixture.caller.organization_id, &fixture.workspace)
        .await;
    observed.push((
        "owner reset",
        fixture
            .host
            .transport
            .clock_responses
            .load(Ordering::SeqCst)
            - before,
    ));
    before = fixture
        .host
        .transport
        .clock_responses
        .load(Ordering::SeqCst);
    let answer = runtime
        .request(
            &fixture.caller,
            WorkspaceRequest {
                request_id: Some(id(7903)),
                workspace_id: Some(fixture.workspace.clone()),
                operation: m::WorkspaceOperation::CreateArea {
                    name: support::name("Reopened clock"),
                },
            },
        )
        .await
        .unwrap();
    observed.push((
        "reopened direct",
        fixture
            .host
            .transport
            .clock_responses
            .load(Ordering::SeqCst)
            - before,
    ));
    let m::ItemRef::Area(reopened) = support::created(answer) else {
        panic!("area missing")
    };
    let stored = runtime.inspect_workspace(&fixture.workspace, Statement {
        sql: "SELECT area_id,created_at FROM areas WHERE area_id IN (?,?) ORDER BY created_at", args: vec![text(&area), text(reopened)],
    }).await.unwrap();
    assert_eq!(stored.rows.len(), 2);
    assert_eq!(stored.rows[0][1], text("2029-01-01T00:00:00.300+00:00"));
    assert_eq!(stored.rows[1][1], text("2029-01-01T00:00:00.400+00:00"));
    fixture.host.close().await;
    eprintln!("successful clock RPC counts: {observed:?}");
    for (operation, count) in observed {
        assert_eq!(count, 1, "{operation}: exactly one clock RPC");
    }
}
