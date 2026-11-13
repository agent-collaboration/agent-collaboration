use super::{WorkLogReads, change_work_log};
use crate::fixtures::{context, id};
use workspace_model::{TrailText, WorkLogBody, WorkLogCommand, WorkLogEntry};
#[test]
fn r8_redaction_clears_body_in_the_mutation_and_keeps_entry() {
    let row = WorkLogEntry {
        entry_id: id(10),
        project_id: id(5),
        author: id(2),
        body: WorkLogBody::Visible(TrailText::try_from("secret".to_owned()).unwrap()),
        tags: vec![],
    };
    let plan = change_work_log(
        &context(),
        &WorkLogCommand::Redact { entry_id: id(10) },
        WorkLogReads {
            project: None,
            entry: Some(&row),
            tag_ids: &[],
        },
    )
    .unwrap();
    assert_eq!(
        plan.batch.statements.first().unwrap().statement.sql,
        "UPDATE work_log_entries SET body=NULL,redacted=1,updated_at=? WHERE entry_id=?"
    );
    assert_eq!(plan.batch.statements.len(), 2);
}
