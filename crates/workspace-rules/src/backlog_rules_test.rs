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
                item: None
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
                item: None
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
                item: Some(&item)
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
