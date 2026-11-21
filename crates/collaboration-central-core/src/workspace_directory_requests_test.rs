use super::show_directory_workspace;
use central_storage::{BatchOutcome, RowExpectation};
use fleet_contracts::WorkspaceDirectory;
use fleet_runtime_ports::workspace_decode_error::corrupt;
use fleet_runtime_test_support::InMemoryWorkspaceDirectory;
use workspace_model as m;

#[tokio::test]
async fn show_reads_the_injected_directory_after_the_caller_applies_its_plan() {
    let directory = InMemoryWorkspaceDirectory::open().await.unwrap();
    let workspace_id: m::WorkspaceId = "019f0000-0000-7000-8000-000000000001".parse().unwrap();
    let name = m::ResourceName::try_from("Research".to_owned()).unwrap();
    let description = m::Description::try_from("Shared notes".to_owned()).unwrap();
    let now = "2027-01-15T08:00:00Z".parse().unwrap();

    let insert = directory
        .plan_guarded_insert(&workspace_id, &name, &description, now)
        .unwrap();
    assert_eq!(insert.expect, RowExpectation::Exactly(1));
    assert_eq!(
        show_directory_workspace(&directory, &workspace_id).await,
        Err(corrupt("directory_workspaces.workspace_id"))
    );
    let outcome = directory.apply_guarded_insert(insert).await.unwrap();
    assert!(
        matches!(outcome, BatchOutcome::Applied { .. }),
        "{outcome:?}"
    );

    let answer = show_directory_workspace(&directory, &workspace_id)
        .await
        .unwrap();
    assert_eq!(
        answer,
        m::WorkspaceReply {
            result: m::WorkspaceResult::Done(m::WorkspaceAnswer::Item {
                item: m::ItemView::Workspace(m::Workspace {
                    workspace_id: workspace_id.clone(),
                    name: name.clone(),
                    description: description.clone(),
                    state: m::WorkspaceState::Active,
                }),
            }),
            actors: vec![],
        }
    );
    let row = directory.read_by_name(&name).await.unwrap().unwrap();
    let view = directory.workspace_view(row).unwrap();
    assert_eq!(view.workspace_id.as_str(), workspace_id.as_str());
    assert_eq!(view.description.as_str(), description.as_str());
}
