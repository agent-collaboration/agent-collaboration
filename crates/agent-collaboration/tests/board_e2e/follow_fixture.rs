//! Shared literal follow membership, range and delivery observations for process tests.
use super::{
    fixture::{BoardFixture, accepted, at, fields, header, identity},
    owned_host::BoardHost,
};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use codex_router_test_support::FixtureResult;
use collaboration_protocol::{PushRecordShowResult, SessionRef};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem};
use message_board::SubscriptionMembership;
use serde_json::{Value, json};
use std::time::Duration;

pub const BOUND: Duration = Duration::from_secs(30);

pub struct FollowMembership {
    pub target: SessionRef,
    pub thread: String,
    pub membership: SubscriptionMembership,
}

impl FollowMembership {
    pub async fn arrange(fixture: &BoardFixture, target: SessionRef) -> FixtureResult<Self> {
        let thread = fixture.thread(&fixture.project, &fixture.owner).await?;
        let actor = json!({"kind":"session","session":target});
        fixture
            .accept(
                "thread_join",
                json!({"requestId":identity(),"protocolVersion":1,"actor":actor,
            "thread":{"project":fixture.project,"thread":thread},"role":"participant",
            "watch":true,"replace":null,"note":null}),
            )
            .await?;
        fixture
            .accept(
                "thread_subscribe",
                fields(
                    header(&actor),
                    json!({
            "scope":{"kind":"thread","thread":{"project":fixture.project,"thread":thread}},
            "policy":{"mode":"deliver","whenIdle":"hold","quietSeconds":0,
                "capSeconds":0,"lifetimeSeconds":86400}}),
                )?,
            )
            .await?;
        let membership = fixture
            .feed_records("subscriptionMembership")
            .await?
            .into_iter()
            .rfind(|row| row.get("reader") == Some(&actor))
            .ok_or("literal subscribed reader missing from public feed")?;
        let membership: SubscriptionMembership = serde_json::from_value(membership)?;
        assert_eq!(membership.generation().get(), 2);
        assert!(membership.ended().is_none());
        Ok(Self {
            target,
            thread,
            membership,
        })
    }

    pub async fn post(&self, fixture: &BoardFixture, text: &str) -> FixtureResult<Value> {
        let post = fixture.reply(&fixture.project, &self.thread, text).await?;
        let shown = fixture
            .host()?
            .mcp("post_show", json!({"postId":at(&post,"/postId")?}))
            .await?;
        let placement: message_board::RecordPlacement =
            serde_json::from_value(at(&shown, "/position")?.clone())?;
        let cursor = placement
            .project_cursor()
            .ok_or("literal post cursor absent")?;
        Ok(json!({"root":self.thread,"from":cursor,"through":cursor,"postCount":1}))
    }

    pub fn assert_frozen(
        &self,
        item: &DeliveryItem<BoardDeliveryTypes>,
        range: &Value,
    ) -> FixtureResult {
        assert_eq!(
            serde_json::to_value(&item.recipient().address)?,
            serde_json::to_value(&self.target)?
        );
        let BoardDeliveryBody::Notification(body) = item.body() else {
            return Err("follow handoff has another body kind".into());
        };
        let source = serde_json::to_value(&body.source)?;
        assert_eq!(source.get("kind"), Some(&json!("followBatch")));
        assert_eq!(
            source.get("membership"),
            Some(&json!(self.membership.envelope.id))
        );
        assert_eq!(
            source.get("activatedAt"),
            Some(&json!(self.membership.activated_at()))
        );
        assert_eq!(source.get("generation"), Some(&json!(2)));
        assert_eq!(source.get("roots"), Some(&json!([range])));
        Ok(())
    }

    pub async fn unsubscribe(
        &self,
        fixture: &BoardFixture,
    ) -> FixtureResult<message_board::FeedCursor> {
        let actor = json!({"kind":"session","session":self.target});
        accepted(&fixture.host()?.mcp("thread_unsubscribe", fields(header(&actor), json!({
            "scope":{"kind":"thread","thread":{"project":fixture.project,"thread":self.thread}}
        }))?).await?)?;
        let ended = fixture
            .feed_records("subscriptionMembership")
            .await?
            .into_iter()
            .rfind(|row| row.pointer("/envelope/id") == Some(&json!(self.membership.envelope.id)))
            .ok_or("ended membership missing from public feed")?;
        let ended: SubscriptionMembership = serde_json::from_value(ended)?;
        assert_eq!(ended.generation(), self.membership.generation());
        assert_eq!(ended.activated_at(), self.membership.activated_at());
        let end = ended.ended().ok_or("unsubscribe did not end membership")?;
        assert_eq!(end.reason, message_board::EndReason::Cancelled);
        Ok(end.position)
    }
}

pub fn only_item(
    items: Vec<DeliveryEnvelope<BoardDeliveryTypes>>,
) -> FixtureResult<DeliveryItem<BoardDeliveryTypes>> {
    let mut items = items.into_iter();
    let Some(DeliveryEnvelope::Item(item)) = items.next() else {
        return Err("expected exactly one actual frozen follow item".into());
    };
    if items.next().is_some() {
        return Err("follow range was allocated more than once".into());
    }
    Ok(item)
}

pub async fn delivered(
    host: &BoardHost,
    target: &SessionRef,
    item: &str,
) -> FixtureResult<PushRecordShowResult> {
    native_state(
        host,
        target,
        item,
        collaboration_protocol::PushDeliveryState::Delivered,
    )
    .await
}

pub async fn native_state(
    host: &BoardHost,
    target: &SessionRef,
    item: &str,
    state: collaboration_protocol::PushDeliveryState,
) -> FixtureResult<PushRecordShowResult> {
    tokio::time::timeout(BOUND, async {
        loop {
            let reply = host
                .mcp_reply("router_show", json!({"caller":target,"reference":item}))
                .await?;
            if !reply.is_error {
                let shown: PushRecordShowResult = serde_json::from_value(reply.value)?;
                if shown.record.delivery_state == state {
                    return Ok(shown);
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

pub async fn sender_status(host: &BoardHost, item: &str, kind: &str) -> FixtureResult<Value> {
    let mut last = None;
    let result = tokio::time::timeout(BOUND, async {
        loop {
            let reply = host
                .mcp_reply(
                    "router_show",
                    json!({
                "caller":{"kind":"humanUser"},"reference":item}),
                )
                .await?;
            if !reply.is_error && reply.value.pointer("/mail/status/kind") == Some(&json!(kind)) {
                assert_eq!(reply.value.pointer("/mail/item"), Some(&json!(item)));
                return Ok(reply.value);
            }
            last = Some(json!({"isError":reply.is_error,"value":reply.value}));
            tokio::task::yield_now().await;
        }
    })
    .await;
    match result {
        Ok(result) => result,
        Err(error) => Err(format!(
            "sender status for {item} never became {kind}: {error}; last observation: {last:?}"
        )
        .into()),
    }
}
