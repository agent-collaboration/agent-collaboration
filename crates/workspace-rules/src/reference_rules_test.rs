use super::{LocatedItem, ReferenceReads, change_reference, references_to};
use crate::fixtures::{context, id};
use central_storage::SqlValue;

#[test]
fn r6_issue_source_returns_the_specific_refusal_before_command_construction() {
    assert_eq!(
        super::reference_source(ItemRef::Issue(id(10))),
        Err(RefusalReason::SourceKindNotAllowed {
            item: ItemRef::Issue(id(10))
        })
    );
    assert!(super::reference_source(ItemRef::Thread(id(8))).is_ok());
}
use workspace_model::{
    ExternalLink, ItemRef, Reference, ReferenceCommand, ReferenceSource, RefusalReason,
};
#[test]
fn r22_a_c_missing_foreign_and_complete_reverse_lookup() {
    let context = context();
    let reference = Reference {
        reference_id: id(10),
        source: ReferenceSource::Thread(id(8)),
        target: ItemRef::Project(id(5)),
        details: None,
    };
    let source = LocatedItem {
        workspace_id: id(1),
        item: ItemRef::Thread(id(8)),
    };
    let mut target = LocatedItem {
        workspace_id: id(99),
        item: ItemRef::Project(id(5)),
    };
    let command = ReferenceCommand::AddReference {
        reference: reference.clone(),
    };
    assert!(matches!(
        change_reference(
            &context,
            &command,
            ReferenceReads {
                source: Some(&source),
                ..Default::default()
            }
        ),
        Err(RefusalReason::NotFound { .. })
    ));
    assert!(matches!(
        change_reference(
            &context,
            &command,
            ReferenceReads {
                source: Some(&source),
                target: Some(&target),
                ..Default::default()
            }
        ),
        Err(RefusalReason::TargetOutsideWorkspace { .. })
    ));
    target.workspace_id = id(1);
    assert!(
        change_reference(
            &context,
            &command,
            ReferenceReads {
                source: Some(&source),
                target: Some(&target),
                ..Default::default()
            }
        )
        .is_ok()
    );
    let other = Reference {
        reference_id: id(11),
        source: ReferenceSource::BacklogItem(id(12)),
        ..reference.clone()
    };
    let unrelated = Reference {
        reference_id: id(13),
        target: ItemRef::Project(id(99)),
        ..reference.clone()
    };
    let rows = [reference, other, unrelated];
    assert_eq!(references_to(&ItemRef::Project(id(5)), &rows).len(), 2);
}
#[test]
fn r2_reference_removal_never_moves_a_project() {
    let reference = Reference {
        reference_id: id(10),
        source: ReferenceSource::Initiative(id(20)),
        target: ItemRef::Project(id(5)),
        details: None,
    };
    let plan = change_reference(
        &context(),
        &ReferenceCommand::RemoveReference {
            reference_id: id(10),
        },
        ReferenceReads {
            reference: Some(&reference),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        plan.batch
            .statements
            .first()
            .unwrap()
            .statement
            .sql
            .starts_with("DELETE FROM item_references")
    );
    assert!(
        plan.batch
            .statements
            .iter()
            .all(|row| !row.statement.sql.contains("UPDATE projects"))
    );
}
#[test]
fn r22_d_links_preserve_valid_and_non_url_strings_exactly() {
    let source = LocatedItem {
        workspace_id: id(1),
        item: ItemRef::Thread(id(8)),
    };
    for url in ["https://example.com/a", "not a URL\n raw text"] {
        let link = ExternalLink {
            link_id: id(10),
            source: ReferenceSource::Thread(id(8)),
            url: url.to_owned(),
            title: "title".to_owned(),
            details: None,
        };
        let plan = change_reference(
            &context(),
            &ReferenceCommand::AddLink { link },
            ReferenceReads {
                source: Some(&source),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            plan.batch
                .statements
                .first()
                .unwrap()
                .statement
                .args
                .contains(&SqlValue::Text(url.to_owned()))
        );
    }
}

#[test]
fn r33_c_closed_initiative_cannot_gain_an_open_project() {
    let mut project = crate::fixtures::project();
    let initiative = workspace_model::Initiative {
        initiative_id: id(20),
        name: crate::fixtures::name("Initiative A"),
        description: crate::fixtures::description("Description"),
        status: workspace_model::WorkStatus::Closed,
    };
    let reference = Reference {
        reference_id: id(10),
        source: ReferenceSource::Initiative(id(20)),
        target: ItemRef::Project(id(5)),
        details: None,
    };
    let source = LocatedItem {
        workspace_id: id(1),
        item: ItemRef::Initiative(id(20)),
    };
    let target = LocatedItem {
        workspace_id: id(1),
        item: ItemRef::Project(id(5)),
    };
    let command = ReferenceCommand::AddReference { reference };
    let result = change_reference(
        &context(),
        &command,
        ReferenceReads {
            source: Some(&source),
            target: Some(&target),
            initiative: Some(&initiative),
            project: Some(&project),
            ..Default::default()
        },
    );
    assert!(
        matches!(result,Err(RefusalReason::ParentClosed {parent}) if parent.title == "Initiative A")
    );
    project.status = workspace_model::WorkStatus::Closed;
    assert!(
        change_reference(
            &context(),
            &command,
            ReferenceReads {
                source: Some(&source),
                target: Some(&target),
                initiative: Some(&initiative),
                project: Some(&project),
                ..Default::default()
            }
        )
        .is_ok()
    );
}
