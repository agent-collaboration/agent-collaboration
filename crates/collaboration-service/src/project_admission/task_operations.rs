use super::{
    operation_context::{OperationContext, OperationPlan},
    work_operations::next_revision,
};
use message_board::{
    BoardOperation, ChangeOutcome, EvidencePointer, LifecycleFact, LinkChange, Record, Refusal,
    SubjectRevision, Subtask, SubtaskId, Task, TaskAction, TaskId, TaskLink, TaskLinkEvent,
    TaskLinkEventId, TaskLinkEventKind, TaskLinkKind, TaskRefusal, TaskStatus, TaskStatusChange,
    TrailRefusal, TrailSubject, TrailText, WorkTrailRow, WorkTrailRowId, work_rules,
};
use message_board_storage::LinkMutation;

impl OperationContext<'_> {
    pub(super) fn task_plan(&self) -> Result<OperationPlan, Refusal> {
        match &self.request.operation {
            BoardOperation::TaskCreate {
                task_id,
                workstream_id,
                title,
                body,
                blocked_by,
                related_to,
            } => {
                self.require_new(task_id.clone().into())?;
                self.require_open_workstream(workstream_id)?;
                let task = Task {
                    envelope: self.envelope(task_id.clone().into()),
                    task_id: task_id.clone(),
                    project_id: self.request.project_id.clone(),
                    workstream_id: workstream_id.clone(),
                    title: title.clone(),
                    body: body.clone(),
                    status: TaskStatus::Todo,
                    revision: SubjectRevision::INITIAL,
                };
                let mut tasks = self.tasks();
                tasks.push(task.clone());
                let mut links = self.links.clone();
                let mut plan = OperationPlan::changed(vec![Record::Task(task)]);
                for (kind, targets) in [
                    (TaskLinkKind::BlockedBy, blocked_by),
                    (TaskLinkKind::RelatedTo, related_to),
                ] {
                    for target in targets {
                        let link = TaskLink {
                            from: task_id.clone(),
                            kind,
                            to: target
                                .task_in_project(&self.request.project_id)
                                .map_err(Refusal::Task)?,
                        };
                        if work_rules::add_task_link(&link, &tasks, &links)
                            .map_err(Refusal::Task)?
                            == LinkChange::Added
                        {
                            links.push(link.clone());
                            plan.records
                                .push(self.task_link_event(TaskLinkEventKind::Added(link.clone())));
                            plan.links.push((link, LinkMutation::Add));
                        }
                    }
                }
                Ok(plan)
            }
            BoardOperation::TaskUpdate {
                task_id,
                title,
                body,
                expected_revision,
            } => {
                let mut task = self.mutable_task(task_id, *expected_revision)?;
                if task.title == *title && task.body == *body {
                    return Ok(OperationPlan::unchanged());
                }
                task.title = title.clone();
                task.body = body.clone();
                Ok(OperationPlan::changed(vec![Record::Task(
                    self.changed_task(task)?,
                )]))
            }
            BoardOperation::TaskMoveWorkstream {
                task_id,
                workstream_id,
                expected_revision,
            } => {
                let mut task = self.mutable_task(task_id, *expected_revision)?;
                self.require_open_workstream(workstream_id)?;
                if task.workstream_id == *workstream_id {
                    return Ok(OperationPlan::unchanged());
                }
                task.workstream_id = workstream_id.clone();
                Ok(OperationPlan::changed(vec![Record::Task(
                    self.changed_task(task)?,
                )]))
            }
            BoardOperation::TaskSetStatus {
                task_id,
                status,
                expected_revision,
            } => {
                let mut task = self.task(task_id)?;
                self.check_task_revision(*expected_revision, task.revision)?;
                if work_rules::task_status_change(task_id, task.status, status.target_status())
                    .map_err(Refusal::Task)?
                    == ChangeOutcome::AlreadySo
                {
                    return Ok(OperationPlan::unchanged());
                }
                task.status = status.target_status();
                let mut records = vec![Record::Task(self.changed_task(task)?)];
                if let TaskStatusChange::Drop { reason } = status {
                    records.push(Record::Fact(self.lifecycle_fact(LifecycleFact::Dropped {
                        task: task_id.clone(),
                        reason: reason.as_str().to_owned(),
                    })));
                }
                Ok(OperationPlan::changed(records))
            }
            BoardOperation::TaskLink { link } => {
                let link = link
                    .link_in_project(&self.request.project_id)
                    .map_err(Refusal::Task)?;
                let result = work_rules::add_task_link(&link, &self.tasks(), &self.links)
                    .map_err(Refusal::Task)?;
                if result == LinkChange::AlreadyPresent {
                    return Ok(OperationPlan::unchanged());
                }
                let task = self.task(&link.from)?;
                let mut plan =
                    OperationPlan::changed(vec![Record::Task(self.record_task_change(task))]);
                plan.records
                    .push(self.task_link_event(TaskLinkEventKind::Added(link.clone())));
                plan.links.push((link, LinkMutation::Add));
                Ok(plan)
            }
            BoardOperation::TaskUnlink { link } => {
                self.task(&link.from)?;
                if work_rules::remove_task_link(link, &self.links) == LinkChange::NotPresent {
                    return Ok(OperationPlan::unchanged());
                }
                let task = self.task(&link.from)?;
                let mut plan =
                    OperationPlan::changed(vec![Record::Task(self.record_task_change(task))]);
                plan.links.push((link.clone(), LinkMutation::Remove));
                plan.records
                    .push(self.task_link_event(TaskLinkEventKind::Removed(link.clone())));
                Ok(plan)
            }
            BoardOperation::TaskAction {
                task_id,
                action,
                expected_revision,
            } => {
                let mut task = self.mutable_task(task_id, *expected_revision)?;
                let (target, note, verb) = match action {
                    TaskAction::Duplicate { of, note } => (of, note, "duplicate of"),
                    TaskAction::Replace { by, note } => (by, note, "replaced by"),
                };
                self.task(target)?;
                task.status = TaskStatus::Dropped;
                let row_id = WorkTrailRowId::generate();
                let text = TrailText::try_from(format!("{verb} {}: {note}", target.as_str()))
                    .map_err(|_| {
                        Refusal::Trail(TrailRefusal::InvalidShape {
                            field: "text".into(),
                        })
                    })?;
                let trail = WorkTrailRow {
                    envelope: self.envelope(row_id.clone().into()),
                    row_id,
                    project_id: self.request.project_id.clone(),
                    subject: TrailSubject::Task(task_id.clone()),
                    text,
                    tags: Vec::new(),
                    links: vec![EvidencePointer::Record {
                        record: target.clone().into(),
                    }],
                    redacted: false,
                };
                Ok(OperationPlan::changed(vec![
                    Record::Task(self.changed_task(task)?),
                    Record::WorkTrailRow(trail),
                    Record::Fact(self.lifecycle_fact(LifecycleFact::Dropped {
                        task: task_id.clone(),
                        reason: format!("{verb} {}: {note}", target.as_str()),
                    })),
                ]))
            }
            BoardOperation::SubtaskAdd {
                subtask_id,
                task_id,
                text,
                place,
            } => {
                self.require_new(subtask_id.clone().into())?;
                self.task(task_id)?;
                Ok(OperationPlan::changed(vec![Record::Subtask(Subtask {
                    envelope: self.envelope(subtask_id.clone().into()),
                    subtask_id: subtask_id.clone(),
                    task_id: task_id.clone(),
                    text: text.clone(),
                    checked: false,
                    removed: false,
                    place: place.clone(),
                })]))
            }
            BoardOperation::SubtaskCheck {
                subtask_id,
                checked,
            } => {
                let mut row = self.subtask(subtask_id)?;
                if row.checked == *checked {
                    return Ok(OperationPlan::unchanged());
                }
                row.checked = *checked;
                row.envelope = self.envelope(subtask_id.clone().into());
                Ok(OperationPlan::changed(vec![Record::Subtask(row)]))
            }
            BoardOperation::SubtaskMove { subtask_id, place } => {
                let mut row = self.subtask(subtask_id)?;
                if row.place == *place {
                    return Ok(OperationPlan::unchanged());
                }
                row.place = place.clone();
                row.envelope = self.envelope(subtask_id.clone().into());
                Ok(OperationPlan::changed(vec![Record::Subtask(row)]))
            }
            BoardOperation::SubtaskRemove { subtask_id } => {
                let mut row = self.subtask(subtask_id)?;
                row.removed = true;
                row.envelope = self.envelope(subtask_id.clone().into());
                let mut plan = OperationPlan::changed(vec![Record::Subtask(row)]);
                plan.removed_subtasks.push(subtask_id.clone());
                Ok(plan)
            }
            _ => Err(Refusal::InvalidShape {
                field: "operation".into(),
            }),
        }
    }
    fn subtask(&self, id: &SubtaskId) -> Result<Subtask, Refusal> {
        if !self.live_subtasks.contains(id) {
            return Err(Refusal::NotFound(id.clone().into()));
        }
        match self.record(&id.clone().into())? {
            Record::Subtask(row) if !row.removed => Ok(row.clone()),
            _ => Err(Refusal::NotFound(id.clone().into())),
        }
    }
    fn tasks(&self) -> Vec<Task> {
        self.records
            .iter()
            .filter_map(|record| match record {
                Record::Task(row) => Some(row.clone()),
                _ => None,
            })
            .collect()
    }
    fn task_link_event(&self, change: TaskLinkEventKind) -> Record {
        let id = TaskLinkEventId::generate();
        Record::TaskLinkEvent(TaskLinkEvent {
            envelope: self.envelope(id.clone().into()),
            id,
            change,
        })
    }
    fn check_task_revision(
        &self,
        expected: SubjectRevision,
        current: SubjectRevision,
    ) -> Result<(), Refusal> {
        if expected == current {
            Ok(())
        } else {
            Err(Refusal::Task(TaskRefusal::StaleRevision { current }))
        }
    }
    fn mutable_task(&self, id: &TaskId, expected: SubjectRevision) -> Result<Task, Refusal> {
        let task = self.task(id)?;
        self.check_task_revision(expected, task.revision)?;
        if task.status == TaskStatus::Dropped {
            return Err(Refusal::Task(TaskRefusal::Closed {
                subject: id.clone().into(),
            }));
        }
        Ok(task)
    }
    fn record_task_change(&self, mut task: Task) -> Task {
        task.envelope = self.envelope(task.task_id.clone().into());
        task
    }
    fn changed_task(&self, mut task: Task) -> Result<Task, Refusal> {
        task.revision = next_revision(task.revision)?;
        Ok(self.record_task_change(task))
    }
}
