use central_storage::{
    AlarmUpdate, GuardedStatement, RowExpectation, SqlValue, Statement, StatementBatch,
};
use workspace_model::{
    ActorIdentityId, ChangeId, ItemRef, RefusalReason, RuleAnswer, Timestamp, WorkspaceId,
};

/// Central supplies these from its admitted caller and injected id/clock sources.
pub struct WriteContext {
    pub workspace_id: WorkspaceId,
    pub actor: ActorIdentityId,
    pub change_id: ChangeId,
    pub now: Timestamp,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RulePlan {
    pub batch: StatementBatch,
    pub answer: RuleAnswer,
}

pub(crate) fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}
pub(crate) fn statement(
    sql: &'static str,
    args: Vec<SqlValue>,
    expect: RowExpectation,
) -> GuardedStatement {
    GuardedStatement {
        statement: Statement { sql, args },
        expect,
    }
}
pub(crate) fn exact(sql: &'static str, args: Vec<SqlValue>) -> GuardedStatement {
    statement(sql, args, RowExpectation::Exactly(1))
}
pub(crate) fn any(sql: &'static str, args: Vec<SqlValue>) -> GuardedStatement {
    statement(sql, args, RowExpectation::Any)
}
pub(crate) fn plan(
    context: &WriteContext,
    item: ItemRef,
    event: &'static str,
    mut statements: Vec<GuardedStatement>,
    answer: RuleAnswer,
) -> RulePlan {
    let (kind, id) = item.kind_and_id();
    let thread = match &item {
        ItemRef::Thread(id) => text(id),
        _ => SqlValue::Null,
    };
    let project = match &item {
        ItemRef::Project(id) => text(id),
        _ => SqlValue::Null,
    };
    statements.push(exact(
        "INSERT INTO changes (change_id,position,change_kind,item_kind,item_id,thread_id,project_id,actor,subject_actor,role,created_at,updated_at) VALUES (?,(SELECT COALESCE(MAX(position),0)+1 FROM changes),?,?,?,?,?,?,?,?,?,?)",
        vec![text(&context.change_id), text(event), text(kind), text(id), thread, project, text(&context.actor), SqlValue::Null, SqlValue::Null, text(context.now.to_rfc3339()), text(context.now.to_rfc3339())]));
    RulePlan {
        batch: StatementBatch {
            statements,
            alarm: AlarmUpdate::Unchanged,
        },
        answer,
    }
}
pub(crate) fn unchanged() -> RulePlan {
    RulePlan {
        batch: StatementBatch {
            statements: vec![],
            alarm: AlarmUpdate::Unchanged,
        },
        answer: RuleAnswer::Unchanged,
    }
}
pub(crate) fn not_found(item: ItemRef) -> RefusalReason {
    RefusalReason::NotFound { item }
}

pub(crate) fn change_metadata(
    mut plan: RulePlan,
    thread: Option<&workspace_model::ThreadId>,
    project: Option<&workspace_model::ProjectId>,
    subject: Option<&ActorIdentityId>,
    role: Option<&str>,
) -> RulePlan {
    if let Some(change) = plan.batch.statements.last_mut() {
        for (index, value) in [
            (4, thread.map_or(SqlValue::Null, text)),
            (5, project.map_or(SqlValue::Null, text)),
            (7, subject.map_or(SqlValue::Null, text)),
            (8, role.map_or(SqlValue::Null, text)),
        ] {
            if let Some(argument) = change.statement.args.get_mut(index) {
                *argument = value;
            }
        }
    }
    plan
}
