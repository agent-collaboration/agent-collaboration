use super::*;
#[tokio::test]
async fn archive_initiative_update_role_release_and_any_member_backlog_edit_use_real_rows() {
    let f = Fixture::new().await;
    let area = f.area(900).await;
    let project = f.project(901, area.clone()).await;
    let deliverable = f.deliverable(902, project).await;
    assert_eq!(
        f.call(
            912,
            m::WorkspaceOperation::CreateIssue {
                deliverable_id: id(999),
                title: "Missing parent".to_owned().try_into().unwrap()
            }
        )
        .await
        .result,
        m::WorkspaceResult::NotFound {
            workspace_id: f.workspace.clone(),
            item: m::ItemRef::Deliverable(id(999))
        }
    );
    assert!(
        serde_json::from_value::<m::ReferenceSource>(
            serde_json::json!({"kind":"issue","id":id::<m::IssueId>(999)})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<m::WorkspaceOperation>(
            serde_json::json!({"kind":"createProject","title":"No area","lead":null})
        )
        .is_err()
    );
    let m::ItemRef::Initiative(initiative) = created(
        f.call(
            903,
            m::WorkspaceOperation::CreateInitiative {
                name: name("Before"),
                description: "Before".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("initiative");
    };
    assert!(matches!(
        f.call(
            904,
            m::WorkspaceOperation::UpdateInitiative {
                initiative_id: initiative.clone(),
                name: name("After"),
                description: "After".to_owned().try_into().unwrap()
            }
        )
        .await
        .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    let shown = f
        .call(
            0,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::Initiative(initiative),
            },
        )
        .await;
    assert!(
        matches!(shown.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Item{item:m::ItemView::Initiative(row)}) if row.name==name("After"))
    );
    f.call(
        905,
        m::WorkspaceOperation::GrantDeliverableRole {
            deliverable_id: deliverable.clone(),
            role: m::DeliverableRole::Reviewer,
            holder: m::ActorRef::Me,
        },
    )
    .await;
    let release = m::WorkspaceOperation::ReleaseDeliverableRole {
        deliverable_id: deliverable.clone(),
        role: m::DeliverableRole::Reviewer,
    };
    assert!(matches!(
        f.call(906, release.clone()).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert_eq!(
        f.call(907, release).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Unchanged {
            item: m::ItemRef::Deliverable(deliverable)
        })
    );
    let m::ItemRef::BacklogItem(item) = created(
        f.call(
            908,
            m::WorkspaceOperation::AddBacklogItem {
                area_id: area.clone(),
                title: name("Before"),
                description: "Before".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("backlog");
    };
    let mut nonlead = f.caller.clone();
    let mut nonlead_session = session(&nonlead);
    nonlead_session.session_id = "nonlead-editor".to_owned().try_into().unwrap();
    nonlead.caller = m::Caller::Session(nonlead_session);
    assert!(matches!(
        call(
            &f.host,
            &nonlead,
            Some(f.workspace.clone()),
            909,
            m::WorkspaceOperation::UpdateBacklogItem {
                item_id: item.clone(),
                title: name("Edited"),
                description: "Edited".to_owned().try_into().unwrap()
            }
        )
        .await
        .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert!(matches!(
        call(
            &f.host,
            &nonlead,
            Some(f.workspace.clone()),
            910,
            m::WorkspaceOperation::RemoveBacklogItem { item_id: item }
        )
        .await
        .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Removed { .. })
    ));
    f.call(
        911,
        m::WorkspaceOperation::ArchiveArea {
            area_id: area.clone(),
        },
    )
    .await;
    let shown = f
        .call(
            0,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::Area(area),
            },
        )
        .await;
    assert!(
        matches!(shown.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Item{item:m::ItemView::Area(row)}) if row.state==m::AreaState::Archived)
    );
    f.host.close().await;
}
