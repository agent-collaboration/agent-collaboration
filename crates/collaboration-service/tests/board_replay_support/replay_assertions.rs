//! Independent source expectations are compared with public records/views, never view helpers.
use super::{board_fixture::TestError, replay_case::*, replay_driver::ReplayBoard};
use message_board::*;
use serde_json::{Value, json};

fn array(value: &Value) -> Result<&Vec<Value>, TestError> {
    value
        .as_array()
        .ok_or_else(|| "expected fixture array".into())
}
fn field<'value>(value: &'value Value, key: &str) -> Result<&'value Value, TestError> {
    value
        .get(key)
        .ok_or_else(|| format!("expected fixture field {key}").into())
}
fn seq(value: &Value) -> Result<usize, TestError> {
    Ok(usize::try_from(value.as_u64().ok_or("event number")?)?)
}
fn event(case: &ReplayCase, number: usize) -> Result<&ReplayEvent, TestError> {
    case.events
        .iter()
        .find(|event| event.seq == number)
        .ok_or_else(|| "source event missing".into())
}
fn references(
    board: &ReplayBoard,
    case: &ReplayCase,
    values: &Value,
) -> Result<Vec<RecordRef>, TestError> {
    array(values)?
        .iter()
        .map(|value| board.reference(case, value))
        .collect()
}
fn body_record<'records>(
    records: &'records [Record],
    reference: &RecordRef,
) -> Result<&'records Record, TestError> {
    records
        .iter()
        .find(|record| record.reference() == *reference)
        .ok_or_else(|| format!("public record missing {reference:?}").into())
}

impl ReplayBoard {
    pub async fn assert_question(
        &mut self,
        case: &ReplayCase,
        question: &ReplayQuestion,
    ) -> Result<(), TestError> {
        match question.ask {
            QuestionKind::Decisions => self.assert_decisions(case, &question.expect).await,
            QuestionKind::Evidence => self.assert_evidence(case, &question.expect).await,
            QuestionKind::State => self.assert_state(case, &question.expect).await,
        }
        .map_err(|error| format!("{} question {}: {error}", case.case, question.via).into())
    }
    #[allow(clippy::panic_in_result_fn)]
    async fn assert_decisions(
        &mut self,
        case: &ReplayCase,
        expected: &Value,
    ) -> Result<(), TestError> {
        let rows = self.trail(true).await?;
        let expected_refs = array(expected)?
            .iter()
            .map(|item| self.reference(case, field(item, "decision_ref")?))
            .collect::<Result<Vec<_>, TestError>>()?;
        assert_eq!(
            rows.iter()
                .map(|row| RecordRef::from(row.row_id.clone()))
                .collect::<Vec<_>>(),
            expected_refs,
            "exact newest-first decision register"
        );
        for (row, item) in rows.iter().zip(array(expected)?) {
            let source = event(case, seq(field(item, "decision_ref")?)?)?;
            assert_eq!(
                row.text.as_str(),
                field(&source.args, "text")?
                    .as_str()
                    .ok_or("source decision text")?
            );
            assert_eq!(
                row.envelope.actor.declared_identity(),
                Some(self.identity(&source.actor)?)
            );
            let tags: Vec<TrailTag> = serde_json::from_value(field(&source.args, "tags")?.clone())?;
            assert_eq!(row.tags, tags);
            let links: Vec<EvidencePointer> =
                serde_json::from_value(self.resolve(field(&source.args, "links")?, "")?)?;
            assert_eq!(row.links, links);
            assert!(
                !field(item, "text")?
                    .as_str()
                    .ok_or("decision summary")?
                    .is_empty()
            );
            if let Some(owner) = item.get("actingFor") {
                assert_eq!(source.acting_for.as_deref(), owner.as_str());
                assert!(
                    row.tags.iter().any(|tag| tag.as_str() == "owner"),
                    "relayed owner decision is labelled"
                );
            }
        }
        Ok(())
    }
    #[allow(clippy::panic_in_result_fn)]
    async fn assert_evidence(
        &mut self,
        case: &ReplayCase,
        expected: &Value,
    ) -> Result<(), TestError> {
        let facts = self.records("fact_list").await?;
        let trail = self.trail(false).await?;
        for item in array(expected)? {
            let number = seq(field(item, "evidence_ref")?)?;
            let reference = self.reference(case, &json!(number))?;
            let source = event(case, number)?;
            assert!(
                !field(item, "what")?
                    .as_str()
                    .ok_or("evidence description")?
                    .is_empty()
            );
            match field(item, "kind")?.as_str().ok_or("evidence kind")? {
                "ci" | "release" | "production" | "commit"
                    if reference.kind == RecordKind::Fact =>
                {
                    let Record::Fact(fact) = body_record(&facts, &reference)? else {
                        return Err("evidence fact type".into());
                    };
                    let resolved = self.resolve(field(&source.args, "fact")?, "")?;
                    let kind: FactKind = serde_json::from_value(field(&resolved, "kind")?.clone())?;
                    assert_eq!(
                        fact.kind, kind,
                        "evidence kind/outcome/evaluator/commit roundtrip"
                    );
                    assert_eq!(
                        fact.links,
                        serde_json::from_value::<Vec<RecordRef>>(
                            field(&resolved, "links")?.clone()
                        )?
                    );
                    assert_eq!(
                        fact.producer_time,
                        field(&resolved, "producerTime")?
                            .as_str()
                            .ok_or("producer time")?
                            .parse::<chrono::DateTime<chrono::Utc>>()?
                    );
                    if let Some(late) = item.get("late") {
                        assert_eq!(json!(fact.late), *late);
                    }
                }
                "test" | "review" | "commit" => {
                    let row = trail
                        .iter()
                        .find(|row| row.row_id.as_str() == reference.id.as_str())
                        .ok_or("evidence trail row")?;
                    assert_eq!(
                        row.text.as_str(),
                        field(&source.args, "text")?
                            .as_str()
                            .ok_or("receipt text")?
                    );
                    assert_eq!(
                        row.links,
                        serde_json::from_value::<Vec<EvidencePointer>>(
                            self.resolve(field(&source.args, "links")?, "")?
                        )?
                    );
                    assert_eq!(
                        row.envelope.actor.declared_identity(),
                        Some(self.identity(&source.actor)?)
                    );
                }
                _ => return Err("unknown evidence expectation".into()),
            }
        }
        Ok(())
    }
    #[allow(clippy::panic_in_result_fn)]
    async fn assert_state(&mut self, case: &ReplayCase, expected: &Value) -> Result<(), TestError> {
        assert!(
            !field(expected, "state")?
                .as_str()
                .ok_or("state description")?
                .is_empty()
        );
        let directory: ProjectShowResult = self
            .read("project_show", json!({"projectId":self.project}))
            .await?;
        assert_eq!(directory.project.id, self.project);
        let feed = self.feed().await?;
        let state = feed
            .iter()
            .rev()
            .find_map(|entry| match &entry.content {
                FeedContent::Shown(record) => match record.as_ref() {
                    Record::Project(state) => Some(state),
                    _ => None,
                },
                _ => None,
            })
            .ok_or("public project state in feed")?;
        if let Some(closed) = expected.get("projectClosed") {
            assert_eq!(json!(state.closed_at.is_some()), *closed);
        }
        if let Some(closed_by) = expected.get("closedBy") {
            let request = self.requests.get(&seq(closed_by)?).ok_or("close request")?;
            let close = feed
                .iter()
                .find_map(|entry| match &entry.content {
                    FeedContent::Shown(record) if entry.request.as_ref() == Some(request) => {
                        match record.as_ref() {
                            Record::Fact(fact) => match fact.kind {
                                FactKind::Lifecycle(LifecycleFact::Closed { position, .. }) => {
                                    Some((entry, position))
                                }
                                _ => None,
                            },
                            _ => None,
                        }
                    }
                    _ => None,
                })
                .ok_or("public closing fact")?;
            let closing = FeedCursor {
                epoch: close.0.epoch,
                position: close.0.position,
            };
            assert_eq!(close.1, closing);
            assert_eq!(
                state.closed_at,
                Some(closing),
                "state closes at the public typed fact's feed position"
            );
        }
        let roles = self.records("role_list").await?;
        let active = roles
            .iter()
            .find_map(|record| match record {
                Record::RoleAssignment(role)
                    if role.role() == AuthorityRole::Lead && role.ended.is_none() =>
                {
                    Some(role)
                }
                _ => None,
            })
            .ok_or("current lead")?;
        let lead_alias = field(expected, "lead")?.as_str().ok_or("lead alias")?;
        assert_eq!(
            active.holder.declared_identity(),
            Some(self.identity(lead_alias)?)
        );
        if let Some(history) = expected.get("leadHistory") {
            let actual: Vec<_> = roles
                .iter()
                .filter_map(|record| match record {
                    Record::RoleAssignment(role) if role.role() == AuthorityRole::Lead => {
                        Some(role)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(actual.len(), array(history)?.len());
            for (role, wanted) in actual.iter().zip(array(history)?) {
                assert_eq!(
                    role.holder.declared_identity(),
                    Some(self.identity(field(wanted, "holder")?.as_str().ok_or("holder")?)?)
                );
                let end = role.ended.as_ref().map(|end| match end {
                    RoleEnd::Released {} => "released",
                    RoleEnd::Replaced {} => "replaced",
                    RoleEnd::StoodDown { .. } => "stoodDown",
                });
                assert_eq!(json!(end), *field(wanted, "end")?);
                if !field(wanted, "endedAt")?.is_null() {
                    assert_eq!(
                        role.envelope.request.as_ref(),
                        self.requests.get(&seq(field(wanted, "endedAt")?)?)
                    );
                }
            }
        }
        let work: WorkView = self
            .read("view_work", json!({"projectId":self.project}))
            .await?;
        if let Some(lanes) = expected.get("workstreams") {
            self.assert_lanes(case, &work, lanes)?;
        }
        if let Some(queue) = expected.get("readyQueue") {
            assert_eq!(
                work.ready_queue
                    .iter()
                    .cloned()
                    .map(RecordRef::from)
                    .collect::<Vec<_>>(),
                references(self, case, queue)?
            );
        }
        if let Some(blocked) = expected.get("blocked") {
            assert_eq!(
                serde_json::to_value(&work.blocked)?,
                self.resolve(blocked, "")?
            );
        }
        let owner: OwnerView = self.read("view_owner", json!({})).await?;
        let wanted_owner = field(expected, "ownerWaiting")?;
        assert_eq!(owner.waiting.len(), array(wanted_owner)?.len());
        for (wait, wanted) in owner.waiting.iter().zip(array(wanted_owner)?) {
            assert_eq!(wait.subject, self.reference(case, field(wanted, "ref")?)?);
            assert_eq!(
                wait.blocks,
                references(self, case, field(wanted, "blocks")?)?
            );
        }
        let owed = field(expected, "owed")?;
        if let Some(by_lead) = owed.as_object() {
            for (alias, wanted) in by_lead {
                self.assert_owed(case, alias, wanted).await?;
            }
        } else {
            self.assert_owed(case, lead_alias, owed).await?;
        }
        let wanted_card = field(expected, "card")?;
        let card:ContinuationCard=self.read("view_card",json!({"projectId":self.project,"actor":self.identity(field(wanted_card,"actor")?.as_str().ok_or("card actor")?)?,"subject":{"kind":"project"}})).await?;
        assert_eq!(field(wanted_card, "subject")?.as_str(), Some("project"));
        assert_eq!(
            card.pending_handbacks,
            references(self, case, field(wanted_card, "pendingHandbacks")?)?
        );
        assert_eq!(
            card.pending_decisions,
            references(self, case, field(wanted_card, "pendingDecisions")?)?
        );
        let wanted_next = field(wanted_card, "nextObligation")?;
        if wanted_next.is_null() {
            assert!(card.next_obligation.is_none());
        } else {
            let next = card.next_obligation.as_ref().ok_or("next obligation")?;
            assert_eq!(
                next.subject,
                self.reference(case, field(wanted_next, "ref")?)?
            );
            assert_eq!(json!(next.obligation), *field(wanted_next, "obligation")?);
        }
        if let Some(handbacks) = expected.get("pendingHandbacks") {
            assert_eq!(card.pending_handbacks, references(self, case, handbacks)?);
        }
        let pending = field(expected, "pending")?;
        let wanted_handbacks = array(pending)?
            .iter()
            .filter(|item| item.get("kind").and_then(Value::as_str) == Some("handback"))
            .map(|item| self.reference(case, field(item, "ref")?))
            .collect::<Result<Vec<_>, TestError>>()?;
        assert_eq!(card.pending_handbacks, wanted_handbacks);
        let wanted_questions = array(pending)?
            .iter()
            .filter(|item| item.get("kind").and_then(Value::as_str) == Some("ownerQuestion"))
            .map(|item| self.reference(case, field(item, "ref")?))
            .collect::<Result<Vec<_>, TestError>>()?;
        assert_eq!(
            owner
                .waiting
                .iter()
                .map(|item| item.subject.clone())
                .collect::<Vec<_>>(),
            wanted_questions
        );
        let wanted_tasks = array(pending)?
            .iter()
            .filter(|item| item.get("kind").and_then(Value::as_str) == Some("openTask"))
            .map(|item| self.reference(case, field(item, "ref")?))
            .collect::<Result<Vec<_>, TestError>>()?;
        if expected.get("workstreams").is_some() {
            assert_eq!(
                work.workstreams
                    .iter()
                    .flat_map(|lane| &lane.tasks)
                    .filter(|task| matches!(task.status, TaskStatus::Todo | TaskStatus::Doing))
                    .map(|task| RecordRef::from(task.id.clone()))
                    .collect::<Vec<_>>(),
                wanted_tasks
            );
        }
        let trail = self.trail(false).await?;
        for pending_item in array(pending)? {
            assert!(
                !field(pending_item, "text")?
                    .as_str()
                    .ok_or("pending description")?
                    .is_empty()
            );
            let reference = self.reference(case, field(pending_item, "ref")?)?;
            match field(pending_item, "kind")?.as_str() {
                Some("handback" | "ownerQuestion") => {
                    let row = trail
                        .iter()
                        .find(|row| row.row_id.as_str() == reference.id.as_str())
                        .ok_or("pending row content")?;
                    let source = event(case, seq(field(pending_item, "ref")?)?)?;
                    assert_eq!(
                        row.text.as_str(),
                        field(&source.args, "text")?
                            .as_str()
                            .ok_or("pending source text")?
                    );
                    assert_eq!(
                        row.envelope.actor.declared_identity(),
                        Some(self.identity(&source.actor)?)
                    );
                }
                Some("openTask") => {
                    let task = work
                        .workstreams
                        .iter()
                        .flat_map(|lane| &lane.tasks)
                        .find(|task| task.id.as_str() == reference.id.as_str())
                        .ok_or("pending task title")?;
                    let source = case
                        .events
                        .iter()
                        .find(|event| {
                            event.op == "task_create"
                                && event.args.get("taskId") == pending_item.get("ref")
                        })
                        .ok_or("pending task source")?;
                    assert_eq!(
                        task.title.as_str(),
                        field(&source.args, "title")?
                            .as_str()
                            .ok_or("source task title")?
                    );
                }
                _ => return Err("unknown pending expectation".into()),
            }
        }
        for key in ["settledHandbacks", "settledOwnerQuestions"] {
            if let Some(settlements) = expected.get(key) {
                for settled in array(settlements)? {
                    let reference = self.reference(case, field(settled, "ref")?)?;
                    let by = seq(field(settled, "settledBy")?)?;
                    let decision = self.reference(case, &json!(by))?;
                    let row = trail
                        .iter()
                        .find(|row| row.row_id.as_str() == decision.id.as_str())
                        .ok_or("settling decision")?;
                    assert!(row.tags.iter().any(|tag| tag.as_str() == "decision"));
                    assert!(row.links.contains(&EvidencePointer::Record {
                        record: reference.clone()
                    }));
                    let expected_author = settled
                        .get("settledByLead")
                        .and_then(Value::as_str)
                        .unwrap_or(&event(case, by)?.actor);
                    assert_eq!(
                        row.envelope.actor.declared_identity(),
                        Some(self.identity(expected_author)?)
                    );
                    assert!(!card.pending_handbacks.contains(&reference));
                    assert!(!owner.waiting.iter().any(|wait| wait.subject == reference));
                }
            }
        }
        self.assert_late_facts(case, expected).await?;
        if let Some(wanted) = expected.get("thread") {
            let reference = self.reference(case, field(wanted, "ref")?)?;
            let thread: ThreadShowResult = self
                .read(
                    "thread_show",
                    json!({"thread":{"thread":reference.id},"reader":{"kind":"anonymous"}}),
                )
                .await?;
            assert_eq!(json!(thread.thread.state), *field(wanted, "state")?);
        }
        Ok(())
    }
    #[allow(clippy::panic_in_result_fn)]
    fn assert_lanes(
        &self,
        case: &ReplayCase,
        work: &WorkView,
        expected: &Value,
    ) -> Result<(), TestError> {
        assert_eq!(work.workstreams.len(), array(expected)?.len());
        for (lane, wanted) in work.workstreams.iter().zip(array(expected)?) {
            assert_eq!(
                RecordRef::from(lane.id.clone()),
                self.reference(case, field(wanted, "ref")?)?
            );
            let state = match &lane.state {
                WorkstreamState::Open {} => "open",
                WorkstreamState::Closed {
                    reason: CloseReason::Done,
                    ..
                } => "closed:done",
                WorkstreamState::Closed {
                    reason: CloseReason::Dropped,
                    ..
                } => "closed:dropped",
            };
            assert_eq!(Some(state), field(wanted, "state")?.as_str());
            for (holder, key) in [
                (&lane.implementer, "implementer"),
                (&lane.verifier, "verifier"),
            ] {
                let expected_holder = field(wanted, key)?
                    .as_str()
                    .map(|alias| self.identity(alias))
                    .transpose()?;
                assert_eq!(
                    holder.as_ref().and_then(RecordedActor::declared_identity),
                    expected_holder
                );
            }
            assert_eq!(lane.tasks.len(), array(field(wanted, "tasks")?)?.len());
            for (task, wanted_task) in lane.tasks.iter().zip(array(field(wanted, "tasks")?)?) {
                assert_eq!(
                    RecordRef::from(task.id.clone()),
                    self.reference(case, field(wanted_task, "ref")?)?
                );
                assert_eq!(json!(task.status), *field(wanted_task, "status")?);
            }
        }
        Ok(())
    }
    #[allow(clippy::panic_in_result_fn)]
    async fn assert_owed(
        &self,
        case: &ReplayCase,
        alias: &str,
        expected: &Value,
    ) -> Result<(), TestError> {
        let load: LeadLoad = self
            .read(
                "view_lead_load",
                json!({"lead":self.recorded_actor(alias)?}),
            )
            .await?;
        assert_eq!(load.owed.len(), array(expected)?.len());
        assert!(load.unavailable_projects.is_empty());
        for (owed, wanted) in load.owed.iter().zip(array(expected)?) {
            assert_eq!(owed.subject, self.reference(case, field(wanted, "ref")?)?);
            assert_eq!(json!(owed.obligation), *field(wanted, "obligation")?);
        }
        Ok(())
    }
    #[allow(clippy::panic_in_result_fn)]
    async fn assert_late_facts(
        &self,
        case: &ReplayCase,
        expected: &Value,
    ) -> Result<(), TestError> {
        let Some(late) = expected.get("lateFacts") else {
            return Ok(());
        };
        let facts = self.records("fact_list").await?;
        let wanted = references(self, case, late)?;
        let actual = facts
            .iter()
            .filter_map(|record| match record {
                Record::Fact(fact) if fact.late => Some(record.reference()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, wanted);
        for reference in references(self, case, field(expected, "notLateFacts")?)? {
            let Record::Fact(fact) = body_record(&facts, &reference)? else {
                return Err("non-late fact".into());
            };
            assert!(!fact.late);
        }
        assert_eq!(expected.get("lateFactsEmitNoIntents"), Some(&json!(true)));
        let intents = self.records("intent_list").await?;
        assert!(intents.iter().all(|record|!matches!(record,Record::NotifyIntent(intent) if wanted.contains(&intent.cause))));
        Ok(())
    }
}
