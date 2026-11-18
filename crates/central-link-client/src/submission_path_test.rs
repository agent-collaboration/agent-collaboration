use super::*;
use central_link_protocol::RequestPayload;
use hegel::{TestCase, generators as gs};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use workspace_model::{WorkspaceRequest, central_mail::CentralMailRequest};

/// Wire `kind` tags the owner put in the outbox (2026-11-15).
const OUTBOX_KINDS: [&str; 2] = ["threadPost", "sendMessage"];

const WORKSPACE_KINDS: [&str; 44] = [
    "createWorkspace",
    "updateWorkspace",
    "archiveWorkspace",
    "createArea",
    "archiveArea",
    "updateArea",
    "createProject",
    "updateProject",
    "createInitiative",
    "updateInitiative",
    "createDeliverable",
    "createIssue",
    "updateIssue",
    "grantProjectRole",
    "releaseProjectRole",
    "grantDeliverableRole",
    "releaseDeliverableRole",
    "createThread",
    "joinThread",
    "addThreadMember",
    "leaveThread",
    "setNotifications",
    "threadPost",
    "resolveThread",
    "reopenThread",
    "addDependency",
    "removeDependency",
    "setIssueStatus",
    "setProjectStatus",
    "setInitiativeStatus",
    "addBacklogItem",
    "updateBacklogItem",
    "removeBacklogItem",
    "closeBacklogItem",
    "addReference",
    "removeReference",
    "addLink",
    "removeLink",
    "workLogAdd",
    "workLogRedact",
    "show",
    "list",
    "inbox",
    "markRead",
];

const MAIL_KINDS: [&str; 6] = [
    "setNoticeCadence",
    "sendMessage",
    "setWake",
    "cancelWake",
    "setAutomation",
    "disableAutomation",
];

fn draw_uuid_v7(case: &TestCase) -> String {
    let millis = case.draw(gs::integers::<u64>().max_value((1 << 48) - 1));
    let random: Vec<u8> = case.draw(gs::vecs(gs::integers::<u8>()).min_size(10).max_size(10));
    let mut bytes = [0_u8; 10];
    bytes.copy_from_slice(&random);
    uuid::Builder::from_unix_timestamp_millis(millis, &bytes)
        .into_uuid()
        .to_string()
}

fn workspace_fields(kind: &str, id: &str, text: &str) -> Value {
    let session_holder = json!({"kind": "me"});
    match kind {
        "createWorkspace" => json!({"name": text}),
        "createArea" => json!({"name": text, "description": text}),
        "archiveArea" => json!({"areaId": id}),
        "updateWorkspace" => json!({"name": text, "description": text}),
        "archiveWorkspace" => json!({}),
        "updateArea" => json!({"areaId": id, "name": text, "description": text}),
        "updateProject" => json!({"projectId": id, "title": text, "description": text}),
        "updateIssue" => json!({"issueId": id, "title": text}),
        "removeLink" => json!({"linkId": id}),
        "createProject" => json!({"areaId": id, "title": text, "description": text, "lead": null}),
        "createInitiative" => json!({"name": text, "description": text}),
        "updateInitiative" => json!({"initiativeId": id, "name": text, "description": text}),
        "createDeliverable" => json!({"projectId": id, "title": text}),
        "createIssue" => json!({"deliverableId": id, "title": text}),
        "grantProjectRole" => json!({"projectId": id, "role": "lead", "holder": session_holder}),
        "releaseProjectRole" => json!({"projectId": id, "role": "advisor"}),
        "grantDeliverableRole" => {
            json!({"deliverableId": id, "role": "implementer", "holder": session_holder})
        }
        "releaseDeliverableRole" => json!({"deliverableId": id, "role": "reviewer"}),
        "createThread" => json!({"areaId": id, "title": text, "body": text, "addressees": []}),
        "joinThread" => json!({"threadId": id, "role": "participant"}),
        "addThreadMember" => json!({"threadId": id, "actor": id, "role": "observer"}),
        "leaveThread" => json!({"threadId": id}),
        "resolveThread" => json!({"threadId": id, "closingPost": text}),
        "reopenThread" => json!({"threadId": id}),
        "setNotifications" => json!({"threadId": id, "setting": "off"}),
        "threadPost" => json!({"threadId": id, "body": text, "addressees": []}),
        "addDependency" => json!({"blocking": id, "blocked": id}),
        "removeDependency" => json!({"dependencyId": id}),
        "setIssueStatus" => json!({"issueId": id, "status": "open"}),
        "setProjectStatus" => json!({"projectId": id, "status": "onHold"}),
        "setInitiativeStatus" => json!({"initiativeId": id, "status": "closed"}),
        "addBacklogItem" => json!({"areaId": id, "title": text, "description": text}),
        "updateBacklogItem" => json!({"itemId": id, "title": text, "description": text}),
        "removeBacklogItem" => json!({"itemId": id}),
        "closeBacklogItem" => json!({"itemId": id, "note": text, "link": null}),
        "addReference" => json!({
            "source": {"kind": "thread", "id": id},
            "target": {"kind": "project", "id": id},
            "details": null,
        }),
        "removeReference" => json!({"referenceId": id}),
        "addLink" => json!({
            "source": {"kind": "post", "id": id},
            "url": "https://example.test/item",
            "title": text,
            "details": null,
        }),
        "workLogAdd" => json!({"projectId": id, "body": text, "tags": []}),
        "workLogRedact" => json!({"entryId": id}),
        "show" => json!({"item": {"kind": "area", "id": id}}),
        "list" => json!({
            "query": {"kind": "areas", "includeArchived": false},
            "cursor": {"order": "oldestFirst", "position": null},
            "limit": 10,
        }),
        "inbox" => json!({
            "scope": {"kind": "area", "areaId": id},
            "readMode": "unread",
            "after": null,
            "limit": 10,
        }),
        "markRead" => json!({"scope": {"kind": "inboxArea", "area_id": id}, "through": 0}),
        other => panic!("no sample for workspace kind {other}"),
    }
}

fn mail_fields(kind: &str, id: &str, text: &str) -> Value {
    let session = json!({"machineId": id, "endpoint": "codex-local", "sessionId": text});
    match kind {
        "setNoticeCadence" => json!({"cadence": "m25"}),
        "sendMessage" => {
            json!({"to": session, "body": text, "delivery": "steer", "replyTo": id, "sentAt": "2026-11-17T10:00:00Z"})
        }
        "setWake" => json!({"target": session, "dueAt": "2026-11-17T10:05:00Z", "text": text}),
        "cancelWake" => json!({"wakeId": id}),
        "setAutomation" => {
            json!({"target": session, "timing": {"kind": "after", "seconds": 300}, "text": text})
        }
        "disableAutomation" => json!({"automationId": id}),
        other => panic!("no sample for mail kind {other}"),
    }
}

fn with_kind(kind: &str, fields: Value) -> Value {
    let mut operation = fields;
    operation["kind"] = json!(kind);
    operation
}

fn workspace_payload(kind: &str, id: &str, text: &str) -> RequestPayload {
    let operation = with_kind(kind, workspace_fields(kind, id, text));
    let request: WorkspaceRequest = serde_json::from_value(json!({
        "requestId": id,
        "workspaceId": id,
        "operation": operation,
    }))
    .unwrap_or_else(|error| panic!("workspace sample {kind} must decode: {error}"));
    RequestPayload::Workspace(request)
}

fn mail_payload(kind: &str, id: &str, text: &str) -> RequestPayload {
    let request: CentralMailRequest = serde_json::from_value(json!({
        "requestId": id,
        "operation": with_kind(kind, mail_fields(kind, id, text)),
    }))
    .unwrap_or_else(|error| panic!("mail sample {kind} must decode: {error}"));
    RequestPayload::Mail(request)
}

#[hegel::test]
fn only_direct_messages_and_thread_posts_go_through_the_outbox(case: TestCase) {
    let id = draw_uuid_v7(&case);
    let text = format!(
        "sample text {}",
        case.draw(gs::integers::<u32>().max_value(1_000_000))
    );
    let (kind, payload) = if case.draw(gs::booleans()) {
        let kind = case.draw(gs::sampled_from(WORKSPACE_KINDS.to_vec()));
        (kind, workspace_payload(kind, &id, &text))
    } else {
        let kind = case.draw(gs::sampled_from(MAIL_KINDS.to_vec()));
        (kind, mail_payload(kind, &id, &text))
    };

    let path = submission_path(&payload);

    let expected = if OUTBOX_KINDS.contains(&kind) {
        SubmissionPath::Outbox
    } else {
        SubmissionPath::Immediate
    };
    assert_eq!(path, expected, "operation {kind}");
}

/// Serde names every variant when it refuses an unknown tag, so the sample
/// lists above cannot silently miss an operation added later.
fn declared_kinds<T: serde::de::DeserializeOwned + std::fmt::Debug>() -> BTreeSet<String> {
    let error = serde_json::from_value::<T>(json!({"kind": "notAnOperation"}))
        .expect_err("an unknown kind must be refused")
        .to_string();
    let listed = error
        .split_once("expected one of ")
        .map(|(_, rest)| rest)
        .unwrap_or_else(|| panic!("serde should list variants: {error}"));
    listed
        .split(", ")
        .map(|name| {
            name.trim_matches(|character: char| character == '`' || !character.is_alphanumeric())
                .to_owned()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

#[test]
fn the_samples_cover_every_declared_operation() {
    let workspace: BTreeSet<String> = WORKSPACE_KINDS
        .iter()
        .map(|kind| (*kind).to_owned())
        .collect();
    let mail: BTreeSet<String> = MAIL_KINDS.iter().map(|kind| (*kind).to_owned()).collect();

    assert_eq!(declared_kinds::<WorkspaceOperation>(), workspace);
    assert_eq!(declared_kinds::<CentralMailOperation>(), mail);
}
