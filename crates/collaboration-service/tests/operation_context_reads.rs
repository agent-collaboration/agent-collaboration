#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Post/task rules load their indexed operands, rather than decoding unrelated current rows.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted, refusal_kind};
use message_board::*;
use message_board_storage::ProjectStorageError;
use serde_json::json;
use sqlx::ConnectOptions;

async fn reencode_record(
    observer: &mut sqlx_turso::TursoConnection,
    record: &Record,
) -> Result<(), TestError> {
    let ended = match record {
        Record::RoleAssignment(role) => role.ended.as_ref().ok_or("ended role")?,
        _ => return Err("unexpected reencoding kind".into()),
    };
    let encoded = serde_json::to_string(&serde_json::to_value(ended)?)?;
    sqlx::query("UPDATE role_assignments SET ended=? WHERE assignment_id=?")
        .bind(encoded)
        .bind(record.reference().id.as_str())
        .execute(observer)
        .await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn, clippy::indexing_slicing)]
async fn admitted_post_and_task_update_read_only_their_rule_operands() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let target = fixture.task(&project, &workstream).await?;
    let unrelated = fixture.task(&project, &workstream).await?;
    let post = PostId::generate();
    let thread = ThreadId::from(post.clone());
    let mut request = fixture.header();
    request["projectId"] = json!(project);
    request["postId"] = json!(post);
    request["about"] = json!({"kind":"workstream","subject":workstream});
    request["role"] = json!(null);
    request["watch"] = json!(false);
    request["text"] = json!("indexed context");
    request["references"] = json!([]);
    request["addressees"] = json!([]);
    accepted(&fixture.api.call("thread_create", request).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let record = read
        .record(&unrelated.clone().into())
        .await?
        .ok_or("unrelated task")?;
    read.finish().await?;
    let mut payload = serde_json::to_value(record)?;
    *payload.pointer_mut("/record/title").ok_or("title")? = json!(" padded unrelated task ");
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    sqlx::query("UPDATE tasks SET title=? WHERE task_id=?")
        .bind(
            payload
                .pointer("/record/title")
                .and_then(|value| value.as_str())
                .ok_or("title")?,
        )
        .bind(unrelated.as_str())
        .execute(&mut observer)
        .await?;
    let mut post_request = fixture.header();
    post_request["postId"] = json!(PostId::generate());
    post_request["placement"] = json!({"kind":"thread","thread":{"thread":thread}});
    post_request["text"] = json!("post without unrelated task decode");
    post_request["references"] = json!([]);
    post_request["addressees"] = json!([]);
    accepted(
        &fixture
            .api
            .call("thread_post", post_request.clone())
            .await?,
    );
    // A fresh request reusing the post still checks the actual post ID.
    post_request["requestId"] = json!(RequestId::generate());
    assert_eq!(
        refusal_kind(&fixture.api.call("thread_post", post_request).await?),
        Some("alreadyExists")
    );
    accepted(&fixture.api.call("task_update", fixture.request(&project, json!({
        "kind":"taskUpdate","taskId":target,"title":"updated target","body":"updated","expectedRevision":0
    }))).await?);
    accepted(&fixture.api.call("workstream_rename", fixture.request(&project, json!({
        "kind":"workstreamRename","workstreamId":workstream,"name":"scoped rename","expectedRevision":0
    }))).await?);
    let mut read = store.snapshot().await?;
    assert!(
        matches!(read.record(&unrelated.clone().into()).await,
        Err(ProjectStorageError::InvalidRecord { field: "title", record }) if record == RecordRef::from(unrelated)),
        "the correction must not normalize or repair the unread corrupt row"
    );
    read.finish().await?;
    sqlx::Connection::close(observer).await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn, clippy::indexing_slicing)]
async fn stale_role_intent_reads_only_the_current_replacement_holder() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let unrelated = WorkstreamId::generate();
    accepted(&fixture.api.call("workstream_create", fixture.request(&project,json!({
        "kind":"workstreamCreate","workstreamId":unrelated,"name":"unrelated roles","goal":"unrelated"
    }))).await?);
    for subject in [&workstream, &unrelated] {
        accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({
            "kind":"roleGrant","subject":{"kind":"workstream","subject":subject},"role":"implementer",
            "holder":{"kind":"human","humanId":"first holder"},"model":null,"lineage":null,"ownerGrant":null,"expectedRevision":0
        }))).await?);
    }
    let cause = PostId::generate();
    let mut root = fixture.header();
    root["projectId"] = json!(project);
    root["postId"] = json!(cause);
    root["about"] = json!({"kind":"workstream","subject":workstream});
    root["text"] = json!("role-addressed cause");
    root["references"] = json!([]);
    root["role"] = json!(null);
    root["watch"] = json!(false);
    root["addressees"] = json!([NotifyTarget::Role(Assignment::Implementer(
        workstream.clone()
    ))]);
    accepted(&fixture.api.call("thread_create", root).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let intent = read
        .records(RecordKind::NotifyIntent)
        .await?
        .into_iter()
        .find_map(|record| match record {
            Record::NotifyIntent(intent) if intent.cause == RecordRef::from(cause.clone()) => {
                Some(intent.intent_id)
            }
            _ => None,
        })
        .ok_or("role intent")?;
    let unrelated_role = read
        .active_role_keys(&RoleSubject::Workstream(unrelated))
        .await?
        .into_iter()
        .next()
        .ok_or("unrelated role")?;
    read.finish().await?;
    accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({
        "kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer",
        "holder":{"kind":"human","humanId":"new holder"},"model":null,"lineage":null,"ownerGrant":null,"expectedRevision":1
    }))).await?);
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    sqlx::query("UPDATE role_assignments SET model=' padded model ' WHERE assignment_id=?")
        .bind(unrelated_role.id.as_str())
        .execute(&mut observer)
        .await?;
    let result = fixture
        ._admission
        .claim_intent(&project, &intent, None)
        .await?;
    assert!(
        matches!(
            &result,
            collaboration_service::IntentReply::Recorded {
                claim: Some(IntentClaim::Superseded { .. }),
                ..
            }
        ),
        "stale role intent must supersede"
    );
    let collaboration_service::IntentReply::Recorded {
        claim: Some(IntentClaim::Superseded { replacements }),
        ..
    } = result
    else {
        return Err("stale role intent did not return a replacement".into());
    };
    let [replacement] = replacements.as_slice() else {
        return Err("expected exactly one replacement".into());
    };
    let replacement = replacement.clone();
    let mut read = store.snapshot().await?;
    let expected_holder = Identity::Human {
        human_id: "new holder".to_owned().try_into()?,
    };
    let replacement = read
        .record(&replacement.into())
        .await?
        .ok_or("replacement")?;
    assert!(
        matches!(replacement,Record::NotifyIntent(intent) if matches!(intent.recipient(),IntentRecipient::Resolved(recipient) if recipient.actor.declared_identity().as_ref() == Some(&expected_holder)))
    );
    assert!(
        read.record(&unrelated_role).await.is_err(),
        "the unrelated corrupt role still fails when requested"
    );
    read.finish().await?;
    sqlx::Connection::close(observer).await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn semantic_identity_matches_survive_json_end_reencoding_and_typed_acknowledgement_readback()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let holder = json!({"kind":"human","humanId":"replacement lead"});
    let grant = OwnerGrantId::generate();
    accepted(&fixture.api.call("owner_grant_record",fixture.request(&project,json!({"kind":"ownerGrantRecord","grantId":grant,"grant":{"kind":"replaceLead","project":project,"newHolder":holder,"reason":"replacement"}}))).await?);
    accepted(&fixture.api.call("role_replace_lead",fixture.request(&project,json!({"kind":"roleReplaceLead","newHolder":holder,"ownerGrant":grant,"reason":"replacement","expectedAssignmentRevision":1}))).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let old = read.records(RecordKind::RoleAssignment).await?.into_iter().find(|record| matches!(record,Record::RoleAssignment(role) if matches!(role.ended,Some(RoleEnd::StoodDown { .. })))).ok_or("stood-down lead")?;
    read.finish().await?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    reencode_record(&mut observer, &old).await?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.record(&old.reference()).await?, Some(old.clone()));
    read.finish().await?;
    let trail = || {
        fixture.request(&project,json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":{"kind":"project"},"text":"old lead write","tags":[],"links":[]}))
    };
    let refused = fixture.api.call("work_trail_add", trail()).await?;
    assert_eq!(
        refusal_kind(&refused),
        Some("standDownUnacknowledged"),
        "{refused}"
    );
    let read = store.snapshot().await?;
    let head = read.high_water();
    read.finish().await?;
    accepted(
        &fixture
            .api
            .call(
                "role_acknowledge_current",
                fixture.request(
                    &project,
                    json!({"kind":"roleAcknowledgeCurrent","readThrough":head}),
                ),
            )
            .await?,
    );
    let mut read = store.snapshot().await?;
    let acknowledgement = read
        .records(RecordKind::StandDownAcknowledgement)
        .await?
        .into_iter()
        .next()
        .ok_or("acknowledgement")?;
    read.finish().await?;
    // Acknowledgement identity now has one typed FK home, with no body blob to reencode.
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.record(&acknowledgement.reference()).await?,
        Some(acknowledgement)
    );
    read.finish().await?;
    accepted(&fixture.api.call("work_trail_add", trail()).await?);
    sqlx::query("UPDATE role_assignments SET ended=json_set(ended,'$.kind','unknownEnd') WHERE assignment_id=?")
        .bind(old.reference().id.as_str()).execute(&mut observer).await?;
    let invalid = fixture.api.call("work_trail_add", trail()).await?;
    assert_eq!(
        invalid.pointer("/error/data/kind"),
        Some(&json!("invalidRecord")),
        "unknown caller role state must fail closed: {invalid}"
    );
    sqlx::Connection::close(observer).await?;
    fixture.stop().await
}
