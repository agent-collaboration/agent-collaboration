use super::*;

fn text_value(value: &str) -> m::Description {
    value.to_owned().try_into().unwrap()
}
fn required(value: &str) -> m::RequiredDescription {
    value.to_owned().try_into().unwrap()
}
fn invalid_description() -> m::WorkspaceResult {
    m::WorkspaceResult::Refused(m::RefusalReason::InvalidInput {
        field: "description".to_owned(),
    })
}
async fn shown_area(f: &Fixture, area: &m::AreaId) -> m::Area {
    let shown = f
        .call(
            0,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::Area(area.clone()),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Item {
        item: m::ItemView::Area(view),
    }) = shown.result
    else {
        panic!("area: {shown:?}");
    };
    view
}
async fn shown_project(f: &Fixture, project: &m::ProjectId) -> m::Project {
    let shown = f
        .call(
            0,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::Project(project.clone()),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Item {
        item: m::ItemView::Project(view),
    }) = shown.result
    else {
        panic!("project: {shown:?}");
    };
    view
}
async fn listed(f: &Fixture, query: m::ListQuery) -> Vec<m::ItemView> {
    let page = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query,
                cursor: m::ListCursor::start(m::ListOrder::OldestFirst),
                limit: m::PageLimit::try_from(50).unwrap(),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Page { items, .. }) = page.result else {
        panic!("page: {page:?}");
    };
    items
}

/// The owner's rule: areas and projects are created with a description that is
/// never blank, and an update may change it but never blank it.
#[tokio::test]
async fn areas_and_projects_require_a_description_that_is_never_blank() {
    let f = Fixture::new().await;

    // A missing description does not decode; a blank one is invalid input.
    for raw in [
        serde_json::json!({"kind":"createArea","name":"No description"}),
        serde_json::json!({"kind":"createProject","areaId":id::<m::AreaId>(1),"title":"No description","lead":null}),
    ] {
        let error = serde_json::from_value::<m::WorkspaceOperation>(raw)
            .unwrap_err()
            .to_string();
        assert!(error.contains("description"), "{error}");
    }
    for (request, blank) in [(1101, ""), (1111, "   \n\t")] {
        assert_eq!(
            f.call(
                request,
                m::WorkspaceOperation::CreateArea {
                    name: name("Blank area"),
                    description: text_value(blank),
                },
            )
            .await
            .result,
            invalid_description()
        );
    }
    let m::ItemRef::Area(area) = created(
        f.call(
            1102,
            m::WorkspaceOperation::CreateArea {
                name: name("Described area"),
                description: text_value("Where release work is planned"),
            },
        )
        .await,
    ) else {
        panic!("area");
    };
    assert_eq!(
        f.call(
            1103,
            m::WorkspaceOperation::CreateProject {
                area_id: area.clone(),
                title: name("Blank project"),
                description: text_value("  "),
                lead: None,
            },
        )
        .await
        .result,
        invalid_description()
    );
    let m::ItemRef::Project(project) = created(
        f.call(
            1104,
            m::WorkspaceOperation::CreateProject {
                area_id: area.clone(),
                title: name("Described project"),
                description: text_value("Ship the description contract"),
                lead: None,
            },
        )
        .await,
    ) else {
        panic!("project");
    };

    // Round trip: Show and List return what was stored.
    assert_eq!(
        shown_area(&f, &area).await.description,
        required("Where release work is planned")
    );
    assert_eq!(
        shown_project(&f, &project).await.description,
        required("Ship the description contract")
    );
    assert!(
        listed(&f, m::ListQuery::Areas { include_archived: false })
            .await
            .iter()
            .any(|item| matches!(item, m::ItemView::Area(row)
                if row.area_id == area && row.description == required("Where release work is planned")))
    );
    assert!(
        listed(&f, m::ListQuery::Projects { area_id: area.clone() })
            .await
            .iter()
            .any(|item| matches!(item, m::ItemView::Project(row)
                if row.project_id == project && row.description == required("Ship the description contract")))
    );

    // An update cannot blank it, and the stored description stays.
    assert_eq!(
        f.call(
            1105,
            m::WorkspaceOperation::UpdateArea {
                area_id: area.clone(),
                name: name("Described area"),
                description: Some(text_value(" ")),
            },
        )
        .await
        .result,
        invalid_description()
    );
    assert_eq!(
        f.call(
            1106,
            m::WorkspaceOperation::UpdateProject {
                project_id: project.clone(),
                title: name("Described project"),
                description: Some(text_value("")),
            },
        )
        .await
        .result,
        invalid_description()
    );
    assert_eq!(
        shown_area(&f, &area).await.description,
        required("Where release work is planned")
    );
    assert_eq!(
        shown_project(&f, &project).await.description,
        required("Ship the description contract")
    );

    // An omitted description keeps it; a new one replaces it.
    for (request, description) in [
        (1107, None),
        (1108, Some(text_value("Plan the next release"))),
    ] {
        f.call(
            request,
            m::WorkspaceOperation::UpdateArea {
                area_id: area.clone(),
                name: name("Renamed area"),
                description,
            },
        )
        .await;
    }
    for (request, description) in [
        (1109, None),
        (1110, Some(text_value("Ship it on both hosts"))),
    ] {
        f.call(
            request,
            m::WorkspaceOperation::UpdateProject {
                project_id: project.clone(),
                title: name("Renamed project"),
                description,
            },
        )
        .await;
    }
    let area_view = shown_area(&f, &area).await;
    assert_eq!(area_view.name, name("Renamed area"));
    assert_eq!(area_view.description, required("Plan the next release"));
    let project_view = shown_project(&f, &project).await;
    assert_eq!(project_view.title, name("Renamed project"));
    assert_eq!(project_view.description, required("Ship it on both hosts"));
    f.host.close().await;
}

/// The title-only update keeps the stored description.
#[tokio::test]
async fn a_rename_without_a_description_keeps_the_stored_one() {
    let f = Fixture::new().await;
    let area = f.area(1120).await;
    let project = f.project(1121, area.clone()).await;
    f.call(
        1122,
        m::WorkspaceOperation::UpdateProject {
            project_id: project.clone(),
            title: name("Renamed only"),
            description: None,
        },
    )
    .await;
    let view = shown_project(&f, &project).await;
    assert_eq!(view.title, name("Renamed only"));
    assert_eq!(view.description, required("Test description"));
    f.host.close().await;
}
