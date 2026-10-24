//! NI-2 integrates native role/intent records with an in-flight claim under the writer guard.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use collaboration_service::IntentReply;
use message_board::*;
use proptest::{
    strategy::{Strategy, ValueTree},
    test_runner::TestRunner,
};
use serde_json::json;
use std::task::Poll;

fn session(name: String) -> Result<Identity, TestError> {
    Ok(Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
                service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            },
            session_id: SessionId::try_from(name)?,
        },
    })
}
#[allow(clippy::panic_in_result_fn)] // Contract assertion; Result reports other fixture failures.
fn claimed(reply: IntentReply) -> Result<ResolvedRecipient, TestError> {
    match reply {
        IntentReply::Recorded {
            claim:
                Some(IntentClaim::Claimed {
                    recipient,
                    replacements,
                }),
            ..
        } => {
            assert!(
                replacements.is_empty(),
                "simple replacement claim must add no further actor"
            );
            Ok(recipient)
        }
        _ => Err("claim did not return a recipient".into()),
    }
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn generated_role_replacements_and_claim_repeats_never_hand_one_cause_to_both_holders()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let mut runner = TestRunner::deterministic();
    let strategy = (proptest::bool::ANY, 0u8..4);
    let mut cases = vec![(false, 0), (true, 0)];
    for _ in 0..16 {
        cases.push(
            strategy
                .new_tree(&mut runner)
                .map_err(|reason| reason.to_string())?
                .current(),
        );
    }
    for (case, (replacement_first, repeats)) in cases.into_iter().enumerate() {
        let (project, workstream) = fixture.project().await?;
        let old_holder = session(format!("old-holder-{case}"))?;
        let new_holder = session(format!("new-holder-{case}"))?;
        accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer","holder":old_holder,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?);
        let post = PostId::generate();
        let mut root = fixture.header();
        root["projectId"] = json!(project);
        root["postId"] = json!(post);
        root["text"] = json!("claim race cause");
        root["about"] = json!({"kind":"workstream","subject":workstream});
        root["role"] = serde_json::Value::Null;
        root["references"] = json!([]);
        root["watch"] = json!(false);
        root["addressees"] = json!([NotifyTarget::Role(Assignment::Implementer(
            workstream.clone()
        ))]);
        accepted(&fixture.api.call("thread_create", root).await?);
        let store = fixture
            ._registry
            .existing_project(&project)
            .await?
            .ok_or("store missing")?;
        let mut snapshot = store.snapshot().await?;
        let original = snapshot
            .records(RecordKind::NotifyIntent)
            .await?
            .into_iter()
            .find_map(|record| match record {
                Record::NotifyIntent(intent) if intent.cause == post.clone().into() => Some(intent),
                _ => None,
            })
            .ok_or("cause intent missing")?;
        let role = snapshot
            .records(RecordKind::RoleAssignment)
            .await?
            .into_iter()
            .find_map(|record| match record {
                Record::RoleAssignment(role)
                    if role.role() == AuthorityRole::Implementer && role.ended.is_none() =>
                {
                    Some(role)
                }
                _ => None,
            })
            .ok_or("current implementer missing")?;
        snapshot.finish().await?;
        let old_recipient = if replacement_first {
            None
        } else {
            Some(claimed(
                fixture
                    ._admission
                    .claim_intent(
                        &project,
                        &original.intent_id,
                        board_fixture::intent_handoff_input()?,
                    )
                    .await?,
            )?)
        };
        // This fixture uses the real typed record writer for a role transition. Authority admission
        // is outside NI-2 and has its own native role tests; no claim/recipient dependency is mocked.
        let mut write = store.begin_admission(fixture.record_origin()?).await?;
        let mut pending = std::pin::pin!(fixture._admission.claim_intent(
            &project,
            &original.intent_id,
            board_fixture::intent_handoff_input()?
        ));
        assert!(
            matches!(futures_util::poll!(pending.as_mut()), Poll::Pending),
            "held writer must prevent claim completion, case {case}"
        );
        let mut previous = role.clone();
        previous.ended = Some(RoleEnd::Replaced {});
        previous.envelope.request = None;
        write
            .append_record(Record::RoleAssignment(previous))
            .await?;
        let assignment = RoleAssignmentId::generate();
        let replacement = RoleAssignment::new(RoleAssignmentProps {
            envelope: RecordEnvelope {
                id: assignment.clone().into(),
                request: None,
                ..role.envelope.clone()
            },
            assignment_id: assignment,
            subject: role.subject(),
            role: role.role(),
            holder: RecordedActor::declared(new_holder.clone()),
            granted_by: role.granted_by.clone(),
            model: role.model.clone(),
            lineage: role.lineage.clone(),
            ended: None,
            owner_grant: None,
        })?;
        write
            .append_record(Record::RoleAssignment(replacement))
            .await?;
        write.commit().await?;
        let result = pending.await?;
        let handed_holder = if replacement_first {
            let replacement = match result {
                IntentReply::Recorded {
                    claim: Some(IntentClaim::Superseded { replacements }),
                    ..
                } => {
                    let [replacement] = replacements.as_slice() else {
                        return Err("one-holder replacement must return exactly one intent".into());
                    };
                    replacement.clone()
                }
                _ => {
                    return Err(
                        format!("case {case}: stale role recipient was not superseded").into(),
                    );
                }
            };
            claimed(
                fixture
                    ._admission
                    .claim_intent(
                        &project,
                        &replacement,
                        board_fixture::intent_handoff_input()?,
                    )
                    .await?,
            )?
        } else {
            assert!(
                matches!(result, IntentReply::Recorded { claim: None, .. }),
                "already handed off cause must not notify a replacement, case {case}"
            );
            old_recipient.ok_or("original claim missing")?
        };
        let expected = if replacement_first {
            &new_holder
        } else {
            &old_holder
        };
        assert_eq!(
            handed_holder.actor.declared_identity().as_ref(),
            Some(expected),
            "case {case}"
        );
        for _ in 0..repeats {
            assert!(
                matches!(
                    fixture
                        ._admission
                        .claim_intent(
                            &project,
                            &original.intent_id,
                            board_fixture::intent_handoff_input()?
                        )
                        .await?,
                    IntentReply::Recorded { claim: None, .. }
                ),
                "repeat claim must not create another handoff, case {case}"
            );
        }
        let mut snapshot = store.snapshot().await?;
        let cause_intents: Vec<_> = snapshot
            .records(RecordKind::NotifyIntent)
            .await?
            .into_iter()
            .filter_map(|record| match record {
                Record::NotifyIntent(intent) if intent.cause == post.clone().into() => Some(intent),
                _ => None,
            })
            .collect();
        assert_eq!(
            cause_intents.len(),
            if replacement_first { 2 } else { 1 },
            "case {case}"
        );
        let handed_count = cause_intents
            .iter()
            .filter(|intent| matches!(intent.state(), IntentState::HandedOff { .. }))
            .count();
        assert_eq!(
            handed_count, 1,
            "one cause may hand off only once, case {case}"
        );
        if replacement_first {
            assert!(
                cause_intents
                    .iter()
                    .any(|intent| intent.intent_id == original.intent_id
                        && matches!(intent.state(), IntentState::Superseded {}))
            );
        }
        snapshot.finish().await?;
    }
    fixture.stop().await
}
