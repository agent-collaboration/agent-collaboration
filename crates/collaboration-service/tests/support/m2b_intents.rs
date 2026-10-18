//! Native arrangements for set re-resolution; no resolver or claim is replaced.
use crate::board_fixture::{BoardFixture, TestError, accepted};
use collaboration_service::IntentReply;
use message_board::*;
use serde_json::{Value, json};

pub fn session(name: &str) -> Result<Identity, TestError> {
    Ok(Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
                endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
            },
            session_id: SessionId::try_from(name.to_owned())?,
        },
    })
}

pub struct IntentFixture {
    pub board: BoardFixture,
    pub project: ProjectId,
    pub workstream: WorkstreamId,
}
impl IntentFixture {
    pub async fn start() -> Result<Self, TestError> {
        let board = BoardFixture::start().await?;
        let (project, workstream) = board.project().await?;
        Ok(Self {
            board,
            project,
            workstream,
        })
    }
    pub async fn grant(&self, role: &str, holder: &Identity) -> Result<(), TestError> {
        let subject = self.role_subject(role);
        let revision = self.subject_revision(&subject).await?;
        accepted(&self.board.api.call("role_grant", self.board.request(&self.project, json!({"kind":"roleGrant","subject":subject,"role":role,"holder":holder,"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))).await?);
        Ok(())
    }
    pub async fn release(&self, role: &str, holder: &Identity) -> Result<(), TestError> {
        let store = self
            .board
            ._registry
            .existing_project(&self.project)
            .await?
            .ok_or("project")?;
        let mut read = store.snapshot().await?;
        let subject = self.role_subject(role);
        let revision = read.assignment_revision(&subject).await?;
        let holder = read
            .records(RecordKind::RoleAssignment)
            .await?
            .into_iter()
            .find_map(|record| match record {
                Record::RoleAssignment(row)
                    if row.role().as_str() == role
                        && row.ended.is_none()
                        && row.holder.declared_identity() == Some(holder) =>
                {
                    Some(row.holder)
                }
                _ => None,
            })
            .ok_or("active holder")?;
        read.finish().await?;
        accepted(&self.board.api.call("role_release", self.board.request(&self.project, json!({"kind":"roleRelease","subject":subject,"role":role,"holder":holder,"expectedRevision":revision}))).await?);
        Ok(())
    }
    fn role_subject(&self, role: &str) -> RoleSubject {
        if matches!(role, "lead" | "advisor") {
            RoleSubject::Project(self.project.clone())
        } else {
            RoleSubject::Workstream(self.workstream.clone())
        }
    }
    pub async fn revision(&self) -> Result<AssignmentRevision, TestError> {
        self.subject_revision(&RoleSubject::Workstream(self.workstream.clone()))
            .await
    }
    async fn subject_revision(
        &self,
        subject: &RoleSubject,
    ) -> Result<AssignmentRevision, TestError> {
        let store = self
            .board
            ._registry
            .existing_project(&self.project)
            .await?
            .ok_or("project")?;
        let mut read = store.snapshot().await?;
        let revision = read.assignment_revision(subject).await?;
        read.finish().await?;
        Ok(revision)
    }
    pub async fn cause(&self, targets: Vec<Value>) -> Result<RecordRef, TestError> {
        let post = PostId::generate();
        let mut request = self.board.header();
        request.as_object_mut().ok_or("request")?.extend(json!({"projectId":self.project,"observedEpoch":1,"postId":post,"text":"set resolution cause","about":{"kind":"workstream","subject":self.workstream},"role":null,"references":[],"watch":false,"addressees":targets}).as_object().ok_or("fields")?.clone());
        accepted(&self.board.api.call("thread_create", request).await?);
        Ok(post.into())
    }
    pub fn role_target(&self, role: &str) -> Value {
        let subject = if matches!(role, "lead" | "advisor") {
            json!(self.project)
        } else {
            json!(self.workstream)
        };
        json!({"kind":"role","recipient":{"kind":role,"subject":subject}})
    }
    pub async fn intents(&self, cause: &RecordRef) -> Result<Vec<NotifyIntent>, TestError> {
        let store = self
            .board
            ._registry
            .existing_project(&self.project)
            .await?
            .ok_or("project")?;
        let mut read = store.snapshot().await?;
        let intents = read
            .records(RecordKind::NotifyIntent)
            .await?
            .into_iter()
            .filter_map(|record| match record {
                Record::NotifyIntent(intent) if intent.cause == *cause => Some(intent),
                _ => None,
            })
            .collect();
        read.finish().await?;
        Ok(intents)
    }
    pub async fn claim(&self, intent: &NotifyIntentId) -> Result<Value, TestError> {
        Ok(
            match self
                .board
                ._admission
                .claim_intent(&self.project, intent)
                .await?
            {
                IntentReply::Recorded { claim, .. } => serde_json::to_value(claim)?,
                IntentReply::Deferred {
                    recipient,
                    replacements,
                } => json!({"kind":"deferred","recipient":recipient,"replacements":replacements}),
            },
        )
    }
}
pub fn actor_intent<'a>(
    intents: &'a [NotifyIntent],
    actor: &Identity,
) -> Result<&'a NotifyIntent, TestError> {
    intents.iter().find(|intent| matches!(intent.recipient(), IntentRecipient::Resolved(recipient) if recipient.actor.declared_identity() == Some(actor))).ok_or_else(|| "actor intent missing".into())
}
