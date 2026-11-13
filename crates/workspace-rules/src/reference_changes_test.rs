use super::{LocatedItem, ReferenceReads, change_reference, reference_source};
use crate::{
    RulePlan,
    fixtures::{context, id},
};
use central_storage::{RowExpectation, SqlValue};
use workspace_model::{
    ExternalLink, ItemKind, ItemRef, Reference, ReferenceCommand, ReferenceSource, RefusalReason,
    RuleAnswer,
};

fn assert_change(
    plan: &RulePlan,
    kind: &str,
    relation_id: &str,
    event: &str,
    thread: SqlValue,
    project: SqlValue,
) {
    let change = plan.batch.statements.last().unwrap();
    assert_eq!(change.expect, RowExpectation::Exactly(1));
    assert_eq!(
        change.statement.args,
        vec![
            SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000003".to_owned()),
            SqlValue::Text(event.to_owned()),
            SqlValue::Text(kind.to_owned()),
            SqlValue::Text(relation_id.to_owned()),
            thread,
            project,
            SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000002".to_owned()),
            SqlValue::Null,
            SqlValue::Null,
            SqlValue::Text("2026-11-13T12:00:00+00:00".to_owned()),
            SqlValue::Text("2026-11-13T12:00:00+00:00".to_owned()),
        ]
    );
}

#[test]
fn relation_lifecycle_names_its_own_id_and_keeps_the_post_thread() {
    let context = context();
    let post = workspace_model::Post {
        post_id: id(30),
        thread_id: id(8),
        author: id(2),
        body: workspace_model::PostText::try_from("Post".to_owned()).unwrap(),
        addressees: vec![],
    };
    let source = LocatedItem {
        workspace_id: id(1),
        item: ItemRef::Post(id(30)),
    };
    let target = LocatedItem {
        workspace_id: id(1),
        item: ItemRef::Project(id(5)),
    };
    let reference = Reference {
        reference_id: id(70),
        source: ReferenceSource::Post(id(30)),
        target: target.item.clone().try_into().unwrap(),
        details: None,
    };
    let added = change_reference(
        &context,
        &ReferenceCommand::AddReference {
            reference: reference.clone(),
        },
        ReferenceReads {
            source: Some(&source),
            target: Some(&target),
            source_post: Some(&post),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        added.answer,
        RuleAnswer::Created {
            item: ItemRef::Reference(id(70))
        }
    );
    assert_eq!(ItemRef::Reference(id(70)).kind(), ItemKind::Reference);
    assert_change(
        &added,
        "reference",
        "01890f2e-7b4c-7cc0-98c4-000000000046",
        "created",
        SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000008".to_owned()),
        SqlValue::Null,
    );
    let removed = change_reference(
        &context,
        &ReferenceCommand::RemoveReference {
            reference_id: id(70),
        },
        ReferenceReads {
            reference: Some(&reference),
            source_post: Some(&post),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        removed.answer,
        RuleAnswer::Removed {
            item: ItemRef::Reference(id(70))
        }
    );
    assert_change(
        &removed,
        "reference",
        "01890f2e-7b4c-7cc0-98c4-000000000046",
        "removed",
        SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000008".to_owned()),
        SqlValue::Null,
    );
    let link = ExternalLink {
        link_id: id(71),
        source: reference.source,
        url: "not a URL".to_owned(),
        title: "Link".to_owned(),
        details: None,
    };
    let added = change_reference(
        &context,
        &ReferenceCommand::AddLink { link: link.clone() },
        ReferenceReads {
            source: Some(&source),
            source_post: Some(&post),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        added.answer,
        RuleAnswer::Created {
            item: ItemRef::Link(id(71))
        }
    );
    assert_eq!(ItemRef::Link(id(71)).kind(), ItemKind::Link);
    assert_change(
        &added,
        "link",
        "01890f2e-7b4c-7cc0-98c4-000000000047",
        "created",
        SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000008".to_owned()),
        SqlValue::Null,
    );
    let removed = change_reference(
        &context,
        &ReferenceCommand::RemoveLink { link_id: id(71) },
        ReferenceReads {
            link: Some(&link),
            source_post: Some(&post),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        removed.answer,
        RuleAnswer::Removed {
            item: ItemRef::Link(id(71))
        }
    );
    assert_change(
        &removed,
        "link",
        "01890f2e-7b4c-7cc0-98c4-000000000047",
        "removed",
        SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000008".to_owned()),
        SqlValue::Null,
    );
}

#[test]
fn work_log_and_deliverable_sources_keep_project_metadata() {
    let context = context();
    let deliverable = crate::fixtures::deliverable(10);
    let entry = workspace_model::WorkLogEntry {
        entry_id: id(20),
        project_id: id(5),
        author: id(2),
        body: workspace_model::WorkLogBody::Redacted,
        tags: vec![],
    };
    for (source, post, log, delivery) in [
        (
            ReferenceSource::WorkLogEntry(id(20)),
            None,
            Some(&entry),
            None,
        ),
        (
            ReferenceSource::Deliverable(id(10)),
            None,
            None,
            Some(&deliverable),
        ),
    ] {
        let located = LocatedItem {
            workspace_id: id(1),
            item: ItemRef::from(source.clone()),
        };
        let link = ExternalLink {
            link_id: id(71),
            source,
            url: "raw".to_owned(),
            title: "Link".to_owned(),
            details: None,
        };
        let output = change_reference(
            &context,
            &ReferenceCommand::AddLink { link: link.clone() },
            ReferenceReads {
                source: Some(&located),
                source_post: post,
                source_work_log: log,
                source_deliverable: delivery,
                ..Default::default()
            },
        )
        .unwrap();
        assert_change(
            &output,
            "link",
            "01890f2e-7b4c-7cc0-98c4-000000000047",
            "created",
            SqlValue::Null,
            SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000005".to_owned()),
        );
        let output = change_reference(
            &context,
            &ReferenceCommand::RemoveLink { link_id: id(71) },
            ReferenceReads {
                link: Some(&link),
                source_work_log: log,
                source_deliverable: delivery,
                ..Default::default()
            },
        )
        .unwrap();
        assert_change(
            &output,
            "link",
            "01890f2e-7b4c-7cc0-98c4-000000000047",
            "removed",
            SqlValue::Null,
            SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000005".to_owned()),
        );
    }
}

#[test]
fn relations_cannot_be_relation_sources_or_targets() {
    for item in [ItemRef::Reference(id(70)), ItemRef::Link(id(71))] {
        assert!(matches!(
            reference_source(item.clone()),
            Err(RefusalReason::SourceKindNotAllowed { .. })
        ));
        assert!(workspace_model::ReferenceTarget::try_from(item).is_err());
    }
}
