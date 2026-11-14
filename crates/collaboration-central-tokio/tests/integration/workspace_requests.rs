//! Real org core requests, actual approved migrations and owning SQLite readback.
use central_storage::{SqlValue, Statement};
use collaboration_central_tokio::TokioWorkspaceHost;
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};
use workspace_model as m;

mod support;
use support::*;

#[tokio::test]
async fn central_replay_and_real_workspace_project_thread_rules() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(HeldClock(AtomicI64::new(1_794_571_200_000)));
    let host = TokioWorkspaceHost::open(root.path(), clock).await.unwrap();
    let caller = caller();
    seed(&host, &caller).await;
    let workspace = create_workspace(&host, &caller).await;
    let operation = m::WorkspaceOperation::CreateArea { name: name("Area") };
    let first = call(
        &host,
        &caller,
        Some(workspace.clone()),
        11,
        operation.clone(),
    )
    .await;
    assert_eq!(
        call(&host, &caller, Some(workspace.clone()), 11, operation).await,
        first
    );
    let m::ItemRef::Area(area_id) = created(first) else {
        panic!("wrong area id");
    };
    let m::ItemRef::Project(project_id) = created(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            12,
            m::WorkspaceOperation::CreateProject {
                area_id: area_id.clone(),
                title: name("Project"),
                lead: None,
            },
        )
        .await,
    ) else {
        panic!("wrong project id");
    };
    let m::ItemRef::Thread(thread_id) = created(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            13,
            m::WorkspaceOperation::CreateThread {
                area_id,
                title: name("Thread"),
            },
        )
        .await,
    ) else {
        panic!("wrong thread id");
    };
    let resolved = call(
        &host,
        &caller,
        Some(workspace.clone()),
        14,
        m::WorkspaceOperation::ResolveThread {
            thread_id: thread_id.clone(),
        },
    )
    .await;
    assert!(matches!(
        resolved.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert_eq!(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            14,
            m::WorkspaceOperation::ResolveThread {
                thread_id: thread_id.clone()
            }
        )
        .await,
        resolved
    );
    assert!(matches!(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            15,
            m::WorkspaceOperation::ResolveThread { thread_id }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::ThreadResolved { .. })
    ));
    assert!(matches!(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            16,
            m::WorkspaceOperation::ReleaseProjectRole {
                project_id,
                role: m::ProjectRole::Lead
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::LeadRequired { .. })
    ));
    host.close().await;
}

#[tokio::test]
async fn admission_checks_real_rows_and_orgs_keep_distinct_central_databases() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(HeldClock(AtomicI64::new(1_794_571_200_000)));
    let host = TokioWorkspaceHost::open(root.path(), clock).await.unwrap();
    let first = caller();
    assert!(matches!(
        call(
            &host,
            &first,
            None,
            10,
            m::WorkspaceOperation::CreateWorkspace {
                name: name("Denied")
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::MachineRevoked)
    ));
    seed(&host, &first).await;
    let workspace = create_workspace(&host, &first).await;
    let mut second = first.clone();
    second.organization_id =
        serde_json::from_str(&format!("\"{}\"", id::<m::WorkspaceId>(99))).unwrap();
    assert!(matches!(
        call(
            &host,
            &second,
            Some(workspace.clone()),
            11,
            m::WorkspaceOperation::CreateArea {
                name: name("Denied")
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::MachineRevoked)
    ));
    seed(&host, &second).await;
    assert!(matches!(
        call(
            &host,
            &second,
            Some(workspace),
            11,
            m::WorkspaceOperation::CreateArea {
                name: name("Denied")
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::WorkspaceNotAllowed)
    ));
    assert_ne!(first.organization_id, second.organization_id);
    host.close().await;
}

#[tokio::test]
async fn real_issue_status_and_computed_views_guard_project_closure() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(HeldClock(AtomicI64::new(1_794_571_200_000)));
    let host = TokioWorkspaceHost::open(root.path(), clock).await.unwrap();
    let caller = caller();
    seed(&host, &caller).await;
    let workspace = create_workspace(&host, &caller).await;
    let m::ItemRef::Area(area_id) = created(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            20,
            m::WorkspaceOperation::CreateArea { name: name("Area") },
        )
        .await,
    ) else {
        panic!("area");
    };
    let m::ItemRef::Project(project_id) = created(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            21,
            m::WorkspaceOperation::CreateProject {
                area_id,
                title: name("Project"),
                lead: None,
            },
        )
        .await,
    ) else {
        panic!("project");
    };
    let m::ItemRef::Deliverable(deliverable_id) = created(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            22,
            m::WorkspaceOperation::CreateDeliverable {
                project_id: project_id.clone(),
                title: name("D"),
            },
        )
        .await,
    ) else {
        panic!("deliverable");
    };
    let close = m::WorkspaceOperation::SetProjectStatus {
        project_id: project_id.clone(),
        status: m::WorkStatus::Closed,
    };
    assert!(matches!(
        call(&host, &caller, Some(workspace.clone()), 23, close.clone())
            .await
            .result,
        m::WorkspaceResult::Refused(m::RefusalReason::ChildrenNotClosed { .. })
    ));
    let m::ItemRef::Issue(issue_id) = created(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            24,
            m::WorkspaceOperation::CreateIssue {
                deliverable_id: deliverable_id.clone(),
                title: "Issue".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("issue");
    };
    for (request, status) in [
        (25, m::WorkStatus::Open),
        (26, m::WorkStatus::OnHold),
        (27, m::WorkStatus::Closed),
    ] {
        assert!(matches!(
            call(
                &host,
                &caller,
                Some(workspace.clone()),
                request,
                m::WorkspaceOperation::SetIssueStatus {
                    issue_id: issue_id.clone(),
                    status
                }
            )
            .await
            .result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ));
    }
    let shown = call(
        &host,
        &caller,
        Some(workspace.clone()),
        0,
        m::WorkspaceOperation::Show {
            item: m::ItemRef::Deliverable(deliverable_id),
        },
    )
    .await;
    assert!(
        matches!(&shown.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Item {item:m::ItemView::Deliverable {view,..}}) if view.status == m::DeliverableStatus::Closed),
        "{shown:?}"
    );
    assert!(matches!(
        call(&host, &caller, Some(workspace.clone()), 28, close)
            .await
            .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert!(matches!(
        call(
            &host,
            &caller,
            Some(workspace),
            29,
            m::WorkspaceOperation::SetIssueStatus {
                issue_id,
                status: m::WorkStatus::Open
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::ParentClosed { .. })
    ));
    host.close().await;
}

#[tokio::test]
async fn lost_post_apply_reply_replays_the_exact_first_resolve_and_expired_ids_are_new() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(HeldClock(AtomicI64::new(1_794_571_200_000)));
    let host = TokioWorkspaceHost::open(root.path(), clock.clone())
        .await
        .unwrap();
    let caller = caller();
    seed(&host, &caller).await;
    let workspace = create_workspace(&host, &caller).await;
    let m::ItemRef::Area(area_id) = created(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            30,
            m::WorkspaceOperation::CreateArea { name: name("Area") },
        )
        .await,
    ) else {
        panic!("area");
    };
    let m::ItemRef::Thread(thread_id) = created(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            31,
            m::WorkspaceOperation::CreateThread {
                area_id,
                title: name("Thread"),
            },
        )
        .await,
    ) else {
        panic!("thread");
    };
    let runtime = host.central(&caller.organization_id).await;
    runtime.lose_reply_after_apply(id(32));
    let operation = m::WorkspaceOperation::ResolveThread {
        thread_id: thread_id.clone(),
    };
    assert_eq!(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            32,
            operation.clone()
        )
        .await
        .result,
        m::WorkspaceResult::OutcomeUnknown
    );
    let applied=runtime.inspect_workspace(&workspace,Statement{sql:"SELECT state,(SELECT json_extract(answer,'$.kind') FROM request_log WHERE request_id=?) AS stored_answer FROM threads WHERE thread_id=?",args:vec![text(id::<m::RequestId>(32)),text(&thread_id)]}).await.unwrap();
    assert_eq!(
        applied.rows,
        vec![vec![text("resolved"), text("done")]],
        "lost answer must follow the actual effect and durable answer"
    );
    let retry = call(
        &host,
        &caller,
        Some(workspace.clone()),
        32,
        operation.clone(),
    )
    .await;
    assert!(matches!(
        retry.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert_eq!(
        call(&host, &caller, Some(workspace.clone()), 32, operation).await,
        retry
    );
    assert!(matches!(
        call(
            &host,
            &caller,
            Some(workspace.clone()),
            32,
            m::WorkspaceOperation::CreateArea {
                name: name("different")
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::RequestIdReused)
    ));
    clock.0.fetch_add(86_400_001, Ordering::SeqCst);
    let recreated = call(
        &host,
        &caller,
        None,
        10,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("After expiry"),
        },
    )
    .await;
    assert!(matches!(
        recreated.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { .. })
    ));
    assert!(matches!(
        call(
            &host,
            &caller,
            Some(workspace),
            32,
            m::WorkspaceOperation::CreateArea {
                name: name("new after idle expiry")
            }
        )
        .await
        .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
    ));
    host.close().await;
}

mod corruption_and_restart;
mod initiatives_and_members;
mod migrations;
mod read_answer_loss;
mod read_projection_cases;
mod relations;
mod remaining_operations;
mod role_and_graph_races;
mod roles_dependencies;
mod turns_and_ledger;
mod workspace_boundaries;

mod relation_positions;

mod admission_negatives;

mod change_ownership;

mod status_and_advisor;
