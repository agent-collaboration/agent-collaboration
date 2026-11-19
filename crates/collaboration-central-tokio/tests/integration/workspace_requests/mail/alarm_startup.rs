//! A failed organization open must not make unrelated native stores unavailable.
use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, CorruptReason, GuardedStatement, RowExpectation, StatementBatch,
    StoreFailure,
};
use collaboration_central_conformance::ConformanceResult;
use collaboration_central_tokio::{ObjectStorage, TokioWorkspaceHost};
use router_clock::RuntimeClock;
use std::sync::Arc;
use workspace_model as m;
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Op, CentralMailRequest,
    CentralMailResult as MailResult,
};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Two physical org stores must reopen independently despite one typed identity failure, in either creation order."
)]
#[tokio::test]
async fn one_corrupt_org_does_not_block_healthy_host_reopen_in_either_creation_order()
-> ConformanceResult<()> {
    for bad_first in [false, true] {
        let root = tempfile::tempdir()?;
        let clock = Arc::new(router_clock::GatedClock::starting_at(
            runtime_primitives::SyncTime::from_unix_millis(1_794_571_200_000),
        )?);
        let keys = signed::SignInKeys::start().await?;
        let host =
            TokioWorkspaceHost::open_with_signin(root.path(), clock.clone(), settings(&keys.url))
                .await?;
        let bad_organization: fleet_identity_protocol::OrganizationId =
            id::<m::WorkspaceId>(8500).to_string().try_into()?;
        // Each organization is provisioned, enrolled and admitted through its real
        // token admission; CreateWorkspace resolves each session through its API.
        let (healthy, bad) = if bad_first {
            let bad = admitted_organization(&host, &bad_organization, clock.as_ref()).await?;
            (
                admitted_organization(&host, &organization(), clock.as_ref()).await?,
                bad,
            )
        } else {
            let healthy = admitted_organization(&host, &organization(), clock.as_ref()).await?;
            (
                healthy,
                admitted_organization(&host, &bad_organization, clock.as_ref()).await?,
            )
        };
        assert_ne!(healthy.organization_id(), bad.organization_id());
        let due = clock.utc_now() + chrono::Duration::seconds(600);
        let wake = host
            .request_central(
                &healthy,
                CentralMailRequest {
                    request_id: id(8501),
                    operation: Op::SetWake {
                        target: session(&healthy),
                        due_at: due,
                        text: "healthy future wake".to_owned().try_into()?,
                    },
                },
            )
            .await?;
        assert!(matches!(wake, MailResult::Done(Answer::WakeSet { .. })));
        let healthy_runtime = host.central(healthy.organization_id()).await?;
        let healthy_before = healthy_runtime
            .statement_host()
            .read(Statement {
                sql: "SELECT id,organization_statement FROM signin_organization_identity ORDER BY id",
                args: vec![],
            })
            .await?
            .rows;
        let bad_runtime = host.central(bad.organization_id()).await?;
        // Host-level recomputation corruption, not a timer policy violation.
        // A second otherwise shaped identity remains invalid even after removing
        // the aggregate timer decoder from recompute_alarm.
        let applied = bad_runtime.statement_host().run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "INSERT INTO signin_organization_identity(id,organization_statement,created_at,updated_at) VALUES (?,'second retained identity',0,0)",
                    args: vec![text(id::<m::WorkspaceId>(8502))],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        }).await?;
        assert!(matches!(applied, BatchOutcome::Applied { .. }));
        let expected = StoreFailure::Corrupt {
            reason: CorruptReason::BadValue {
                column: "signin_organization_identity.id".to_owned(),
            },
        };
        assert_eq!(bad_runtime.recompute_alarm().await, Err(expected.clone()));
        let bad_before = bad_runtime
            .statement_host()
            .read(Statement {
                sql: "SELECT id,organization_statement FROM signin_organization_identity ORDER BY id",
                args: vec![],
            })
            .await?
            .rows;
        assert_eq!(bad_before.len(), 2);
        host.close().await;
        drop(healthy_runtime);
        drop(bad_runtime);
        drop(host);

        // One reopen, no retry or repair: directory enumeration order must not
        // determine whether a healthy organization can finish host startup.
        let reopened =
            TokioWorkspaceHost::open_with_signin(root.path(), clock.clone(), settings(&keys.url))
                .await?;
        let healthy_runtime = reopened.central(healthy.organization_id()).await?;
        assert_eq!(
            healthy_runtime
                .statement_host()
                .alarm()
                .map(|at| at.unix_millis()),
            Some(due.timestamp_millis()),
            "healthy active wake must re-arm during cold startup"
        );
        assert!(
            healthy_runtime
                .statement_host()
                .read(Statement {
                    sql: "SELECT id,organization_statement FROM signin_organization_identity ORDER BY id",
                    args: vec![],
                })
                .await?
                .rows
                == healthy_before
        );
        // Implementations may retain a failed runtime or leave it unopened. In
        // either case its owning recompute boundary must retain the typed reason.
        let bad_error = match reopened.central(bad.organization_id()).await {
            Err(error) => error,
            Ok(runtime) => match runtime.recompute_alarm().await {
                Err(error) => error,
                Ok(()) => return Err("corrupt organization unexpectedly recomputed".into()),
            },
        };
        assert_eq!(bad_error, expected);
        let request = CentralMailRequest {
            request_id: id(8503),
            operation: Op::SendMessage {
                to: session(&healthy),
                body: "healthy after another org failed open"
                    .to_owned()
                    .try_into()?,
                delivery: workspace_model::central_mail::MessageDelivery::Auto,
                reply_to: None,
                sent_at: "2100-01-01T00:00:00Z".parse().unwrap(),
            },
        };
        let answer = reopened.request_central(&healthy, request.clone()).await?;
        assert!(matches!(answer, MailResult::Done(Answer::Queued { .. })));
        assert_eq!(reopened.request_central(&healthy, request).await?, answer);
        let rows = healthy_runtime.statement_host().read(Statement {
            sql: "SELECT (SELECT COUNT(*) FROM mail_messages WHERE message_kind='direct'),(SELECT COUNT(*) FROM runtime_request_log WHERE request_id=?)",
            args: vec![text(id::<m::RequestId>(8503))],
        }).await?.rows;
        assert_eq!(
            rows,
            vec![vec![
                central_storage::SqlValue::Integer(1_i64.try_into()?),
                central_storage::SqlValue::Integer(1_i64.try_into()?),
            ]]
        );
        assert_eq!(
            healthy_runtime
                .statement_host()
                .alarm()
                .map(|at| at.unix_millis()),
            Some(due.timestamp_millis())
        );
        // Direct owning readback verifies that host isolation did not silently
        // clear, coerce or replace the failed organization's durable identity.
        let bad_store = ObjectStorage::new(
            root.path()
                .join("central")
                .join(format!("{}.sqlite", bad.organization_id().as_str())),
            Arc::new(collaboration_central_core::workspace_migrations::central_migrator()),
        );
        assert!(
            bad_store
                .read(Statement {
                    sql: "SELECT id,organization_statement FROM signin_organization_identity ORDER BY id",
                    args: vec![],
                })
                .await?
                .rows
                == bad_before
        );
        bad_store.close().await;
        reopened.close().await;
    }
    Ok(())
}
