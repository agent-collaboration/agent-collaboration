use super::{BacklogReads, change_backlog};
use crate::fixtures::{area, context, description, id, name};
use workspace_model::{BacklogCommand, BacklogItem, RefusalReason};
#[test]
fn r21_a_b_any_workspace_actor_edits_and_description_is_required() {
    let context = context();
    let area = area();
    let item = BacklogItem {
        item_id: id(10),
        area_id: area.area_id.clone(),
        title: name("Item"),
        description: workspace_model::BacklogDescription::try_from("Details".to_owned()).unwrap(),
        creator: id(99),
        status: workspace_model::BacklogStatus::Open,
        closing_note: None,
    };
    let add = |description| BacklogCommand::Add {
        item_id: id(10),
        area_id: area.area_id.clone(),
        title: name("Item"),
        description,
    };
    assert_eq!(
        change_backlog(
            &context,
            &add(description("")),
            BacklogReads {
                area: Some(&area),
                item: None,
                ..Default::default()
            }
        )
        .unwrap_err(),
        RefusalReason::EmptyDescription
    );
    assert!(
        change_backlog(
            &context,
            &add(description("Details")),
            BacklogReads {
                area: Some(&area),
                item: None,
                ..Default::default()
            }
        )
        .is_ok()
    );
    assert!(
        change_backlog(
            &context,
            &BacklogCommand::Update {
                item_id: id(10),
                title: name("Changed"),
                description: description("Updated")
            },
            BacklogReads {
                area: None,
                item: Some(&item),
                ..Default::default()
            }
        )
        .is_ok()
    );
    let output = change_backlog(
        &context,
        &BacklogCommand::Remove { item_id: id(10) },
        BacklogReads {
            area: None,
            item: Some(&item),
            ..Default::default()
        },
    )
    .unwrap();
    let cleanup = output.batch.statements.first().unwrap();
    assert_eq!(
        cleanup.statement.sql,
        "DELETE FROM item_references WHERE (source_kind='backlogItem' AND source_id=?) OR (target_kind='backlogItem' AND target_id=?)"
    );
    assert!(
        output
            .batch
            .statements
            .iter()
            .any(|row| row.statement.sql == "DELETE FROM backlog_items WHERE item_id=?")
    );
}

#[test]
fn r21_close_requires_open_item_note_and_a_same_workspace_reference() {
    use workspace_model::{
        BacklogClosingReference, BacklogStatus, ItemRef, Reference, ReferenceSource,
        ReferenceTarget,
    };
    let context = context();
    let mut item = BacklogItem {
        item_id: id(10),
        area_id: id(4),
        title: name("Backlog"),
        description: "Details".to_owned().try_into().unwrap(),
        creator: id(99),
        status: BacklogStatus::Open,
        closing_note: None,
    };
    let target = ItemRef::Deliverable(id(11));
    let link = BacklogClosingReference {
        change_id: id(13),
        reference: Reference {
            reference_id: id(12),
            source: ReferenceSource::BacklogItem(id(10)),
            target: ReferenceTarget::try_from(target.clone()).unwrap(),
            details: None,
        },
    };
    let command = BacklogCommand::Close {
        item_id: id(10),
        note: description("Done"),
        link: Some(link),
    };
    let located = crate::LocatedItem {
        workspace_id: id(99),
        item: target.clone(),
    };
    assert_eq!(
        change_backlog(
            &context,
            &command,
            BacklogReads {
                item: Some(&item),
                target: Some(&located),
                ..Default::default()
            }
        )
        .unwrap_err(),
        RefusalReason::NotFound {
            item: target.clone()
        }
    );
    let located = crate::LocatedItem {
        workspace_id: context.workspace_id.clone(),
        item: target,
    };
    let plan = change_backlog(
        &context,
        &command,
        BacklogReads {
            item: Some(&item),
            target: Some(&located),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        plan.batch.statements.len(),
        4,
        "close, reference, reference-created, backlog-state change share a batch"
    );
    item.status = BacklogStatus::Closed;
    item.closing_note = Some("Done".to_owned());
    assert_eq!(
        change_backlog(
            &context,
            &command,
            BacklogReads {
                item: Some(&item),
                target: Some(&located),
                ..Default::default()
            }
        )
        .unwrap_err(),
        RefusalReason::BacklogItemClosed { item_id: id(10) }
    );
}
