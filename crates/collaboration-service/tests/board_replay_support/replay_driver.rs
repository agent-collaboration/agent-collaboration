//! Only input revisions/anchors use native snapshots; every mutation uses the public MCP tool.
use super::{
    board_fixture::{BoardFixture, TestError},
    replay_case::*,
};
use message_board::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub struct ReplayBoard {
    pub fixture: BoardFixture,
    pub project: ProjectId,
    pub identities: BTreeMap<String, Identity>,
    pub ids: BTreeMap<String, String>,
    pub cursors: BTreeMap<usize, FeedCursor>,
    pub requests: BTreeMap<usize, RequestId>,
}
impl ReplayBoard {
    pub async fn start(case: &ReplayCase) -> Result<Self, TestError> {
        let fixture = BoardFixture::start_replay().await?;
        let project = ProjectId::generate();
        let identities = case
            .actors
            .iter()
            .map(|actor| {
                let identity = match actor.kind {
                    ReplayActorKind::Human => Identity::Human {
                        human_id: HumanId::try_from(actor.alias.clone())?,
                    },
                    ReplayActorKind::Session => Identity::Session {
                        session: SessionRef {
                            endpoint: SessionEndpointRef {
                                service_id: ServiceId::try_from(
                                    "00000000-0000-4000-8000-000000000001".to_owned(),
                                )?,
                                endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
                            },
                            session_id: SessionId::try_from(
                                PostId::generate().as_str().to_owned(),
                            )?,
                        },
                    },
                };
                Ok((actor.alias.clone(), identity))
            })
            .collect::<Result<BTreeMap<_, _>, TestError>>()?;
        let ids = BTreeMap::from([
            (
                "$workspace".to_owned(),
                fixture.workspace.as_str().to_owned(),
            ),
            ("$board".to_owned(), fixture.board.as_str().to_owned()),
            ("$project".to_owned(), project.as_str().to_owned()),
        ]);
        let mut board = Self {
            fixture,
            project,
            identities,
            ids,
            cursors: BTreeMap::new(),
            requests: BTreeMap::new(),
        };
        let actor = case
            .events
            .first()
            .ok_or("case has no events")?
            .actor
            .clone();
        for setup in &case.setup {
            let args = board.resolve(&setup.args, "")?;
            board.call_operation(&actor, None, &setup.op, args).await?;
        }
        Ok(board)
    }
    pub fn identity(&self, alias: &str) -> Result<&Identity, TestError> {
        self.identities
            .get(alias)
            .ok_or_else(|| format!("unknown actor {alias}").into())
    }
    pub fn recorded_actor(&self, alias: &str) -> Result<RecordedActor, TestError> {
        Ok(RecordedActor {
            actor: ActorKind::Declared(self.identity(alias)?.clone()),
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            machine: MachineLabel::try_from("V1/V2 proof".to_owned())?,
        })
    }
    pub fn resolve(&mut self, value: &Value, field: &str) -> Result<Value, TestError> {
        Ok(match value {
            Value::String(text) if text == "$current" => value.clone(),
            Value::String(text) if text.starts_with("$cursorOf:") => {
                let seq: usize = text.trim_start_matches("$cursorOf:").parse()?;
                json!(
                    self.cursors
                        .get(&seq)
                        .ok_or("causal anchor event missing")?
                )
            }
            Value::String(text) if text.starts_with('$') => {
                let id = self
                    .ids
                    .entry(text.clone())
                    .or_insert_with(|| PostId::generate().as_str().to_owned());
                json!(id)
            }
            Value::String(text) if text.starts_with('@') => {
                let alias = text.trim_start_matches('@');
                if matches!(field, "newHolder" | "recordedBy" | "actor") {
                    json!(self.recorded_actor(alias)?)
                } else {
                    json!(self.identity(alias)?)
                }
            }
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|item| self.resolve(item, field))
                    .collect::<Result<_, _>>()?,
            ),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), self.resolve(value, key)?)))
                    .collect::<Result<_, TestError>>()?,
            ),
            _ => value.clone(),
        })
    }
    #[allow(clippy::panic_in_result_fn)]
    pub async fn replay(&mut self, event: &ReplayEvent) -> Result<(), TestError> {
        let args = self.resolve(&event.args, "")?;
        let (reply, request) = self
            .call_operation(&event.actor, event.acting_for.as_deref(), &event.op, args)
            .await
            .map_err(|error| format!("event {} {}: {error}", event.seq, event.op))?;
        assert!(
            matches!(
                reply.result,
                RequestResult::Decided(RequestDecision::Accepted { .. })
            ),
            "event {} {}: {:?}",
            event.seq,
            event.op,
            reply.result
        );
        self.requests.insert(event.seq, request);
        if let Some(reference) = self.event_reference(event)? {
            let store = self
                .fixture
                ._registry
                .existing_project(&self.project)
                .await?
                .ok_or("project")?;
            let mut snapshot = store.snapshot().await?;
            if let Some(record) = snapshot.record(&reference).await? {
                let cursor = match &record {
                    Record::Project(project) if event.op == "project_close" => project.closed_at,
                    _ => match record.envelope().position {
                        RecordPosition::Project(cursor) => Some(cursor),
                        RecordPosition::Fleet(_) | RecordPosition::Unsequenced => None,
                    },
                };
                if let Some(cursor) = cursor {
                    self.cursors.insert(event.seq, cursor);
                }
            }
            snapshot.finish().await?;
        }
        Ok(())
    }
    pub fn event_reference(&self, event: &ReplayEvent) -> Result<Option<RecordRef>, TestError> {
        let (field, kind) = match event.op.as_str() {
            "project_create" | "project_close" => return Ok(Some(self.project.clone().into())),
            "work_trail_add" => ("rowId", RecordKind::WorkTrailRow),
            "fact_record" => {
                let placeholder = event
                    .args
                    .pointer("/fact/factId")
                    .and_then(Value::as_str)
                    .ok_or("fact ID")?;
                return Ok(Some(RecordRef {
                    kind: RecordKind::Fact,
                    id: RecordId::try_from(
                        self.ids.get(placeholder).ok_or("fact binding")?.clone(),
                    )?,
                }));
            }
            "thread_create" => ("postId", RecordKind::Post),
            "task_create" | "task_set_status" | "task_move_workstream" => {
                ("taskId", RecordKind::Task)
            }
            "workstream_create" | "workstream_close" => ("workstreamId", RecordKind::Workstream),
            _ => return Ok(None),
        };
        let placeholder = event
            .args
            .get(field)
            .and_then(Value::as_str)
            .ok_or("record placeholder")?;
        Ok(Some(RecordRef {
            kind,
            id: RecordId::try_from(self.ids.get(placeholder).ok_or("record binding")?.clone())?,
        }))
    }
    async fn current_revision_value(&self, op: &str, args: &Value) -> Result<Value, TestError> {
        let store = self
            .fixture
            ._registry
            .existing_project(&self.project)
            .await?
            .ok_or("project")?;
        let mut snapshot = store.snapshot().await?;
        let revision = match op {
            "role_grant" => {
                json!(
                    snapshot
                        .assignment_revision(&serde_json::from_value(
                            args.get("subject").cloned().ok_or("role subject")?,
                        )?)
                        .await?
                )
            }
            "role_replace_lead" => {
                json!(
                    snapshot
                        .assignment_revision(&RoleSubject::Project(self.project.clone()))
                        .await?
                )
            }
            _ => {
                let reference = match op {
                    "project_close" => self.project.clone().into(),
                    "workstream_close" => WorkstreamId::try_from(
                        args.get("workstreamId")
                            .and_then(Value::as_str)
                            .ok_or("workstream")?
                            .to_owned(),
                    )?
                    .into(),
                    "task_set_status" | "task_move_workstream" => TaskId::try_from(
                        args.get("taskId")
                            .and_then(Value::as_str)
                            .ok_or("task")?
                            .to_owned(),
                    )?
                    .into(),
                    _ => return Err(format!("revision not defined for {op}").into()),
                };
                match snapshot
                    .record(&reference)
                    .await?
                    .ok_or("revision subject")?
                {
                    Record::Project(record) => json!(record.revision),
                    Record::Workstream(record) => json!(record.revision),
                    Record::Task(record) => json!(record.revision),
                    _ => return Err("unexpected revision subject".into()),
                }
            }
        };
        snapshot.finish().await?;
        Ok(revision)
    }
    pub async fn call_operation(
        &self,
        actor: &str,
        acting_for: Option<&str>,
        op: &str,
        mut args: Value,
    ) -> Result<(WriteReply, RequestId), TestError> {
        let revision_field = if op == "role_replace_lead" {
            "expectedAssignmentRevision"
        } else {
            "expectedRevision"
        };
        if args.get(revision_field).and_then(Value::as_str) == Some("$current") {
            let revision = self.current_revision_value(op, &args).await?;
            args.as_object_mut()
                .ok_or("operation args")?
                .insert(revision_field.to_owned(), json!(revision));
        }
        let request = RequestId::generate();
        let mut envelope = json!({"requestId":request,"protocolVersion":1,"resubmits":null,"actor":self.identity(actor)?,"actingFor":acting_for.map(|alias|self.identity(alias)).transpose()?});
        let operation_kind = match op {
            "role_grant" => Some("roleGrant"),
            "role_replace_lead" => Some("roleReplaceLead"),
            "owner_grant_record" => Some("ownerGrantRecord"),
            "workstream_create" => Some("workstreamCreate"),
            "workstream_close" => Some("workstreamClose"),
            "task_create" => Some("taskCreate"),
            "task_set_status" => Some("taskSetStatus"),
            "task_move_workstream" => Some("taskMoveWorkstream"),
            "task_link" => Some("taskLink"),
            "work_trail_add" => Some("workTrailAdd"),
            "fact_record" => Some("factRecord"),
            "project_close" => Some("projectClose"),
            "workspace_create" | "board_create" | "project_create" | "thread_create"
            | "thread_resolve" => None,
            _ => return Err(format!("unknown case operation {op}").into()),
        };
        if let Some(kind) = operation_kind {
            args.as_object_mut()
                .ok_or("operation")?
                .insert("kind".to_owned(), json!(kind));
            let fields = envelope.as_object_mut().ok_or("envelope")?;
            fields.insert("projectId".to_owned(), json!(self.project));
            fields.insert("observedEpoch".to_owned(), Value::Null);
            fields.insert("operation".to_owned(), args);
        } else {
            envelope
                .as_object_mut()
                .ok_or("envelope")?
                .extend(args.as_object().ok_or("plain args")?.clone());
            if matches!(op, "thread_create" | "thread_resolve") {
                envelope
                    .as_object_mut()
                    .ok_or("envelope")?
                    .insert("observedEpoch".to_owned(), Value::Null);
            }
            if op == "thread_create" {
                envelope
                    .as_object_mut()
                    .ok_or("envelope")?
                    .insert("projectId".to_owned(), json!(self.project));
            }
        }
        let response = self.fixture.api.call(op, envelope).await?;
        let reply = serde_json::from_value(
            response
                .get("result")
                .cloned()
                .ok_or_else(|| format!("{op}: {response}"))?,
        )?;
        Ok((reply, request))
    }
    pub async fn stop(self) -> Result<(), TestError> {
        self.fixture.stop().await
    }

    pub async fn read<TResponse: serde::de::DeserializeOwned>(
        &self,
        tool: &str,
        request: Value,
    ) -> Result<TResponse, TestError> {
        let response = self.fixture.api.call(tool, request).await?;
        Ok(serde_json::from_value(
            response
                .get("result")
                .cloned()
                .ok_or_else(|| format!("{tool}: {response}"))?,
        )?)
    }
    pub async fn records(&self, tool: &str) -> Result<Vec<Record>, TestError> {
        let mut cursor = None;
        let mut records = Vec::new();
        loop {
            let page: RecordListResult = self
                .read(
                    tool,
                    json!({"projectId":self.project,"page":{"limit":100,"cursor":cursor}}),
                )
                .await?;
            records.extend(page.records);
            cursor = page.next_cursor;
            if cursor.is_none() {
                return Ok(records);
            }
        }
    }
    pub async fn feed(&self) -> Result<Vec<FeedEntry>, TestError> {
        let mut after = FeedCursor {
            epoch: EpochNumber::try_from(1)?,
            position: FeedPosition::ZERO,
        };
        let mut entries = Vec::new();
        loop {
            let reply: BoardReadReply<FeedRead> = self.read("feed_read",json!({"projectId":self.project,"reader":{"kind":"anonymous"},"after":after,"limit":100,"includeRedacted":true})).await?;
            let FeedRead::Entries {
                entries: page,
                covered_through,
                high_water,
            } = reply.value
            else {
                return Err("replay feed superseded".into());
            };
            entries.extend(page);
            if covered_through == high_water {
                return Ok(entries);
            }
            if covered_through.position <= after.position {
                return Err("feed failed to advance".into());
            }
            after = covered_through;
        }
    }
    pub async fn trail(&self, decisions: bool) -> Result<Vec<WorkTrailRow>, TestError> {
        let selection = if decisions {
            json!({"kind":"decisions","after":null,"before":null,"rangeTop":null})
        } else {
            json!({"kind":"forward","after":{"epoch":1,"position":0}})
        };
        let reply: BoardReadReply<WorkTrailReadResult> = self.read("work_trail_show",json!({"projectId":self.project,"reader":{"kind":"anonymous"},"selection":selection,"limit":100,"subject":null,"tags":[],"includeRedacted":false})).await?;
        match reply.value {
            WorkTrailReadResult::Forward(page) => Ok(page.rows),
            WorkTrailReadResult::Decisions(page) if page.older_than.is_none() => Ok(page.rows),
            _ => Err("unexpected replay trail continuation".into()),
        }
    }
    pub fn reference(&self, case: &ReplayCase, value: &Value) -> Result<RecordRef, TestError> {
        if let Some(seq) = value.as_u64() {
            let event = case
                .events
                .iter()
                .find(|event| event.seq as u64 == seq)
                .ok_or("expected event")?;
            return self
                .event_reference(event)?
                .ok_or_else(|| "expected event has no named record".into());
        }
        let placeholder = value.as_str().ok_or("expected reference")?;
        let kind = if placeholder.starts_with("$ws.") {
            RecordKind::Workstream
        } else if placeholder.starts_with("$task.") {
            RecordKind::Task
        } else if placeholder.starts_with("$row.") {
            RecordKind::WorkTrailRow
        } else if placeholder.starts_with("$fact.") {
            RecordKind::Fact
        } else if placeholder.starts_with("$post.") {
            RecordKind::Thread
        } else {
            return Err("unknown reference kind".into());
        };
        Ok(RecordRef {
            kind,
            id: RecordId::try_from(
                self.ids
                    .get(placeholder)
                    .ok_or("expected ID binding")?
                    .clone(),
            )?,
        })
    }
}
