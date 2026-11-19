//! Real org core requests, actual approved migrations and owning SQLite readback.
use central_storage::{SqlValue, Statement};
use std::sync::Arc;
use workspace_model as m;

mod support;
use support::*;

#[tokio::test]
async fn central_replay_and_real_workspace_project_thread_rules() {
    let f = Fixture::new().await;
    let host = &f.host;
    let caller = f.caller.clone();
    let workspace = f.workspace.clone();

    let operation = m::WorkspaceOperation::CreateArea {
        name: name("Area"),
        description: "Test description".to_owned().try_into().unwrap(),
    };
    let first = call(
        host,
        &caller,
        Some(workspace.clone()),
        11,
        operation.clone(),
    )
    .await;
    assert_eq!(
        call(host, &caller, Some(workspace.clone()), 11, operation).await,
        first
    );
    let m::ItemRef::Area(area_id) = created(first) else {
        panic!("wrong area id");
    };
    let m::ItemRef::Project(project_id) = created(
        call(
            host,
            &caller,
            Some(workspace.clone()),
            12,
            m::WorkspaceOperation::CreateProject {
                area_id: area_id.clone(),
                title: name("Project"),
                lead: None,
                description: "Test description".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("wrong project id");
    };
    let m::ItemRef::Thread(thread_id) = created(
        call(
            host,
            &caller,
            Some(workspace.clone()),
            13,
            m::WorkspaceOperation::CreateThread {
                area_id,
                title: name("Thread"),
                body: "First post".to_owned().try_into().unwrap(),
                addressees: vec![],
            },
        )
        .await,
    ) else {
        panic!("wrong thread id");
    };
    let resolved = call(
        host,
        &caller,
        Some(workspace.clone()),
        14,
        m::WorkspaceOperation::ResolveThread {
            thread_id: thread_id.clone(),
            closing_post: None,
        },
    )
    .await;
    assert!(matches!(
        resolved.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert_eq!(
        call(
            host,
            &caller,
            Some(workspace.clone()),
            14,
            m::WorkspaceOperation::ResolveThread {
                thread_id: thread_id.clone(),
                closing_post: None,
            }
        )
        .await,
        resolved
    );
    assert!(matches!(
        call(
            host,
            &caller,
            Some(workspace.clone()),
            15,
            m::WorkspaceOperation::ResolveThread {
                thread_id,
                closing_post: None,
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::ThreadResolved { .. })
    ));
    assert!(matches!(
        call(
            host,
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
    use runtime_primitives::Clock;
    let f = Fixture::new().await;
    let now = f.clock.now().unix_millis().div_euclid(1000);
    let runtime = f.host.central(f.caller.organization_id()).await.unwrap();
    let missing = signed::machine_token(
        organization().as_str(),
        &id::<workspace_ids::MachineId>(98).to_string(),
        &id::<m::WorkspaceId>(97).to_string(),
        now,
        now + 600,
    )
    .unwrap();
    assert!(
        runtime
            .admit_machine_connection(&missing)
            .await
            .unwrap()
            .is_err()
    );
    let other: fleet_identity_protocol::OrganizationId =
        id::<m::WorkspaceId>(99).to_string().try_into().unwrap();
    let second_runtime = f.host.central(&other).await.unwrap();
    second_runtime
        .provision_organization(
            &collaboration_central_core::identity_contract::ProvisionOrganization {
                organization_id: other.clone(),
                organization_statement: signed::stamp(other.as_str(), now - 3600)
                    .unwrap()
                    .try_into()
                    .unwrap(),
            },
        )
        .await
        .unwrap();
    let token = signed::machine_token(
        other.as_str(),
        &id::<workspace_ids::MachineId>(2).to_string(),
        &id::<m::WorkspaceId>(4).to_string(),
        now,
        now + 600,
    )
    .unwrap();
    assert!(
        second_runtime
            .admit_machine_connection(&token)
            .await
            .unwrap()
            .is_err()
    );
    seed_machine_for(&f.host, &other, &id(2), 4, now).await;
    let connection = second_runtime
        .admit_machine_connection(&token)
        .await
        .unwrap()
        .unwrap();
    let second =
        collaboration_central_core::WorkspaceCaller::new(connection, f.caller.caller().clone())
            .unwrap();
    assert!(matches!(
        call(
            &f.host,
            &second,
            Some(f.workspace.clone()),
            11,
            m::WorkspaceOperation::CreateArea {
                name: name("Denied"),
                description: "Test description".to_owned().try_into().unwrap(),
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::WorkspaceNotAllowed)
    ));
    assert_ne!(f.caller.organization_id(), second.organization_id());
    f.host.close().await;
}

#[tokio::test]
async fn real_issue_status_and_computed_views_guard_project_closure() {
    let f = Fixture::new().await;
    let host = &f.host;
    let caller = f.caller.clone();
    let workspace = f.workspace.clone();

    let m::ItemRef::Area(area_id) = created(
        call(
            host,
            &caller,
            Some(workspace.clone()),
            20,
            m::WorkspaceOperation::CreateArea {
                name: name("Area"),
                description: "Test description".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("area");
    };
    let m::ItemRef::Project(project_id) = created(
        call(
            host,
            &caller,
            Some(workspace.clone()),
            21,
            m::WorkspaceOperation::CreateProject {
                area_id,
                title: name("Project"),
                lead: None,
                description: "Test description".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("project");
    };
    let m::ItemRef::Deliverable(deliverable_id) = created(
        call(
            host,
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
        call(host, &caller, Some(workspace.clone()), 23, close.clone())
            .await
            .result,
        m::WorkspaceResult::Refused(m::RefusalReason::ChildrenNotClosed { .. })
    ));
    let m::ItemRef::Issue(issue_id) = created(
        call(
            host,
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
                host,
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
        host,
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
        call(host, &caller, Some(workspace.clone()), 28, close)
            .await
            .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert!(matches!(
        call(
            host,
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
    let f = Fixture::new().await;
    let host = &f.host;
    let mut caller = f.caller.clone();
    let workspace = f.workspace.clone();
    let clock = f.clock.clone();

    let m::ItemRef::Area(area_id) = created(
        call(
            host,
            &caller,
            Some(workspace.clone()),
            30,
            m::WorkspaceOperation::CreateArea {
                name: name("Area"),
                description: "Test description".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("area");
    };
    let m::ItemRef::Thread(thread_id) = created(
        call(
            host,
            &caller,
            Some(workspace.clone()),
            31,
            m::WorkspaceOperation::CreateThread {
                area_id,
                title: name("Thread"),
                body: "First post".to_owned().try_into().unwrap(),
                addressees: vec![],
            },
        )
        .await,
    ) else {
        panic!("thread");
    };
    let runtime = host.central(caller.organization_id()).await.unwrap();
    runtime.lose_reply_after_apply(id(32));
    let operation = m::WorkspaceOperation::ResolveThread {
        thread_id: thread_id.clone(),
        closing_post: None,
    };
    assert_eq!(
        call(
            host,
            &caller,
            Some(workspace.clone()),
            32,
            operation.clone()
        )
        .await
        .result,
        m::WorkspaceResult::OutcomeUnknown
    );
    let applied=runtime.inspect_workspace(&workspace,Statement{sql:"SELECT state,(SELECT json_extract(answer,'$.kind') FROM workspace_request_log WHERE request_id=?) AS stored_answer FROM workspace_threads WHERE thread_id=?",args:vec![text(id::<m::RequestId>(32)),text(&thread_id)]}).await.unwrap();
    assert_eq!(
        applied.rows,
        vec![vec![text("resolved"), text("done")]],
        "lost answer must follow the actual effect and durable answer"
    );
    let retry = call(
        host,
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
        call(host, &caller, Some(workspace.clone()), 32, operation).await,
        retry
    );
    assert!(matches!(
        call(
            host,
            &caller,
            Some(workspace.clone()),
            32,
            m::WorkspaceOperation::CreateArea {
                name: name("different"),
                description: "Test description".to_owned().try_into().unwrap(),
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::RequestIdReused)
    ));
    clock
        .advance(std::time::Duration::from_millis(86_400_001))
        .await
        .unwrap();
    caller = f.fresh_caller(&caller).await;
    let recreated = call(
        host,
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
            host,
            &caller,
            Some(workspace),
            32,
            m::WorkspaceOperation::CreateArea {
                name: name("new after idle expiry"),
                description: "Test description".to_owned().try_into().unwrap(),
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
mod request_status;
mod role_and_graph_races;
mod roles_dependencies;
mod turns_and_ledger;
mod workspace_boundaries;

mod contract_additions;
mod relation_positions;

mod admission_negatives;

mod change_ownership;

mod status_and_advisor;

mod lead_taking;

mod descriptions;

mod review_read_boundaries;

mod page_limits;

mod mail;

mod notices;

mod owner_changes;

mod backlog_boundaries;

mod engine_v2_support;

pub(crate) mod engine_evidence;
