//! Native capture and the actual owner path establish each confirmed prefix.
use super::*;
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_conformance::{
    AdmittedPeerSocket, ConformanceClient, enroll_admitted_peer,
};
use collaboration_service::{IntentHandoffInput, IntentRecipientEnrollment};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, PeerId};
use message_board_storage::MailHandoffItem;

#[path = "messaging_rule_machine.rs"]
mod rule_machine;
use rule_machine::RuleMachine;
#[path = "messaging_cancel_fixture.rs"]
mod cancel_fixture;
#[path = "messaging_rule_publication.rs"]
mod publication;
pub(super) use cancel_fixture::FollowSeed;

pub(super) const RECIPIENT: &str = "550e8400-e29b-41d4-a716-446655445183";
pub(super) struct RuleFixture {
    pub holder: claim_fixture::ClaimFixture,
    pub successor: RuleMachine,
    pub recipient: AdmittedPeerSocket,
    pub project: ProjectId,
    pub successor_origin: ActorOrigin,
}
impl RuleFixture {
    pub async fn open(
        holder_notifications: bool,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let holder = if holder_notifications {
            Box::pin(claim_fixture::ClaimFixture::start_with_notifications()).await?
        } else {
            claim_fixture::ClaimFixture::start().await?
        };
        let project = ProjectId::generate();
        project_claim::imported_snapshot(
            &holder.central,
            &holder.directory.path().join("source.sqlite"),
            &project,
        )
        .await?;
        assert!(
            matches!(holder.claim(&project).await?, board_sync_protocol::ProjectClaimReply::Claimed { epoch, .. } if epoch.value()==2)
        );
        let successor_origin = ActorOrigin {
            installation: "550e8400-e29b-41d4-a716-446655445182"
                .to_owned()
                .try_into()?,
            machine: "rule successor".to_owned().try_into()?,
        };
        let successor = Box::pin(RuleMachine::open(&holder, &successor_origin)).await?;
        let client = ConformanceClient::new(holder.central.base_url())?;
        let recipient = enroll_admitted_peer(
            &client,
            RECIPIENT,
            "019f0000-0000-7000-8000-000000051083",
            "rule-mailbox-recipient",
            "019f0000-0000-7000-8000-000000061083",
        )
        .await?;
        let fixture = Self {
            holder,
            successor,
            recipient,
            project,
            successor_origin,
        };
        fixture.grant_writer().await?;
        fixture.confirm_prefix(fixture.head().await?).await?;
        Ok(fixture)
    }
    fn writer() -> Result<Identity, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Identity::Human {
            human_id: "rule cause writer".to_owned().try_into()?,
        })
    }
    async fn write(&self, operation: BoardOperation) -> TestResult {
        let request = ProjectRequest {
            request_id: RequestId::generate(),
            project_id: self.project.clone(),
            actor: Self::writer()?,
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation,
        };
        assert!(matches!(
            self.holder.admission.submit(&request).await?.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ));
        Ok(())
    }
    async fn grant_writer(&self) -> TestResult {
        self.write(BoardOperation::RoleGrant {
            subject: RoleSubject::Project(self.project.clone()),
            role: AuthorityRole::Lead,
            holder: Self::writer()?,
            model: None,
            lineage: None,
            expected_revision: AssignmentRevision::INITIAL,
            owner_grant: None,
        })
        .await
    }
    pub async fn post_cause(&self) -> Result<PostId, Box<dyn std::error::Error + Send + Sync>> {
        let post = PostId::generate();
        self.write(serde_json::from_value(json!({"kind":"threadCreate","postId":post,
            "about":{"kind":"project","subject":{"line":"messaging rule"}},
            "text":"rule cause survives takeover","references":[],"participation":"participant","watch":false,
            "addressees":[{"kind":"actor","recipient":{"kind":"session","session":{
                "endpoint":{"serviceId":RECIPIENT,"endpointId":"codex-local"},"sessionId":"rule recipient"
            }}}]}))?).await?;
        Ok(post)
    }
    pub async fn head(&self) -> Result<FeedCursor, Box<dyn std::error::Error + Send + Sync>> {
        let store = self
            .holder
            .registry
            .existing_project(&self.project)
            .await?
            .ok_or("holder store")?;
        let read = store.snapshot().await?;
        let head = read.high_water();
        read.finish().await?;
        Ok(head)
    }
    pub async fn confirm_prefix(&self, head: FeedCursor) -> TestResult {
        timeout(BOUND, async {
            loop {
                self.holder
                    .projects
                    .replicate_project_for_test(&self.project)
                    .await?;
                if self
                    .holder
                    .projects
                    .replicated_through(&self.project, head.epoch)
                    .await?
                    .position
                    >= head.position
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                tokio::task::yield_now().await;
            }
        })
        .await?
    }
    pub async fn freeze(
        &self,
        post: &PostId,
    ) -> Result<DeliveryItem<BoardDeliveryTypes>, Box<dyn std::error::Error + Send + Sync>> {
        let store = self
            .holder
            .registry
            .existing_project(&self.project)
            .await?
            .ok_or("holder store")?;
        let mut read = store.snapshot().await?;
        let intent = read
            .records(RecordKind::NotifyIntent)
            .await?
            .into_iter()
            .find_map(|record| match record {
                Record::NotifyIntent(intent) if intent.cause == post.clone().into() => Some(intent),
                _ => None,
            })
            .ok_or("cause intent")?;
        read.finish().await?;
        let item = ItemId::try_from("019f0000-0000-7000-8000-000000071081".to_owned())?;
        self.holder
            .admission
            .claim_intent(
                &self.project,
                &intent.intent_id,
                Some(IntentHandoffInput {
                    item,
                    enrollment: IntentRecipientEnrollment {
                        service: RECIPIENT.to_owned().try_into()?,
                        peer: PeerId::new(self.recipient.peer)?,
                    },
                }),
            )
            .await?;
        let mut read = store.snapshot().await?;
        let handoff = read
            .mail_handoff(&item.as_uuid().to_string().try_into()?)
            .await?
            .ok_or("old holder frozen handoff")?;
        read.finish().await?;
        let MailHandoffItem::Delivery(envelope) = handoff.item else {
            return Err("old holder froze a cancel group".into());
        };
        let DeliveryEnvelope::Item(frozen) = *envelope else {
            return Err("old holder froze a control".into());
        };
        assert_eq!(frozen.id(), item);
        Ok(frozen)
    }
    pub async fn close(self) -> TestResult {
        let _ = self
            .holder
            .central
            .runtime_control(
                ObjectAddress::Connection(origin()?.installation),
                RuntimeControl::ResumeDispatch,
            )
            .await;
        self.recipient
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        self.successor.close().await?;
        self.holder.shutdown().await
    }
}
