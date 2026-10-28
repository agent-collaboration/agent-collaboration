use super::*;
use serde_json::json;

const PROJECT_ID: &str = "019a1234-5678-7000-8000-000000000001";
const REQUEST_ID: &str = "019a1234-5678-7000-8000-000000000002";

fn request(operation: Value) -> Value {
    json!({
        "requestId": REQUEST_ID, "projectId": PROJECT_ID,
        "actor": {"kind":"human", "humanId":"schema-parity"},
        "actingFor":null,
        "protocolVersion":1, "operation":operation
    })
}

#[test]
fn project_tool_schemas_refuse_other_tools_operations() {
    let server = CollaborationMcpServer::catalog_only();
    let tools = server.resolved_tools();
    let cases = [
        (
            "workstream_create",
            json!({"kind":"workstreamCreate","workstreamId":PROJECT_ID,"name":"work","goal":"goal"}),
        ),
        (
            "task_create",
            json!({"kind":"taskCreate","taskId":PROJECT_ID,"workstreamId":PROJECT_ID,"title":"task","body":"body","blockedBy":[],"relatedTo":[]}),
        ),
    ];
    for (tool_name, accepted) in &cases {
        let tool = tools.iter().find(|tool| tool.name == *tool_name).unwrap();
        let schema = Value::Object((*tool.input_schema).clone());
        let validator = jsonschema::validator_for(&schema).unwrap();
        assert!(
            validator.is_valid(&request(accepted.clone())),
            "{tool_name} rejected its valid operation"
        );
        for (other_tool, forbidden) in &cases {
            if other_tool != tool_name {
                assert!(
                    !validator.is_valid(&request(forbidden.clone())),
                    "{tool_name} advertises {other_tool}'s operation"
                );
            }
        }
    }
}

fn operation_cases() -> Vec<(&'static str, Value)> {
    let actor = json!({"kind":"human","humanId":"schema-parity"});
    let cursor = json!({"epoch":1,"position":0});
    let subject = json!({"kind":"project","subject":PROJECT_ID});
    let about = json!({"kind":"project","subject":{"line":"project context"}});
    let scope = json!({"kind":"project","projectId":PROJECT_ID});
    vec![
        (
            "workstream_create",
            json!({"kind":"workstreamCreate","workstreamId":PROJECT_ID,"name":"work","goal":"goal"}),
        ),
        (
            "workstream_rename",
            json!({"kind":"workstreamRename","workstreamId":PROJECT_ID,"name":"work","expectedRevision":0}),
        ),
        (
            "workstream_close",
            json!({"kind":"workstreamClose","workstreamId":PROJECT_ID,"reason":"done","note":"complete","expectedRevision":0}),
        ),
        (
            "task_create",
            json!({"kind":"taskCreate","taskId":PROJECT_ID,"workstreamId":PROJECT_ID,"title":"task","body":"body","blockedBy":[],"relatedTo":[]}),
        ),
        (
            "task_update",
            json!({"kind":"taskUpdate","taskId":PROJECT_ID,"title":"task","body":"body","expectedRevision":0}),
        ),
        (
            "task_move_workstream",
            json!({"kind":"taskMoveWorkstream","taskId":PROJECT_ID,"workstreamId":PROJECT_ID,"expectedRevision":0}),
        ),
        (
            "task_set_status",
            json!({"kind":"taskSetStatus","taskId":PROJECT_ID,"status":{"kind":"doing"},"expectedRevision":0}),
        ),
        (
            "task_link",
            json!({"kind":"taskLink","link":{"from":PROJECT_ID,"kind":"relatedTo","to":{"project":PROJECT_ID,"task":REQUEST_ID}}}),
        ),
        (
            "task_unlink",
            json!({"kind":"taskUnlink","link":{"from":PROJECT_ID,"kind":"relatedTo","to":REQUEST_ID}}),
        ),
        (
            "task_duplicate",
            json!({"kind":"taskAction","taskId":PROJECT_ID,"action":{"kind":"duplicate","of":REQUEST_ID,"note":"duplicate"},"expectedRevision":0}),
        ),
        (
            "task_replace",
            json!({"kind":"taskAction","taskId":PROJECT_ID,"action":{"kind":"replace","by":REQUEST_ID,"note":"replace"},"expectedRevision":0}),
        ),
        (
            "subtask_add",
            json!({"kind":"subtaskAdd","subtaskId":PROJECT_ID,"taskId":PROJECT_ID,"text":"subtask","place":"80"}),
        ),
        (
            "subtask_check",
            json!({"kind":"subtaskCheck","subtaskId":PROJECT_ID,"checked":true}),
        ),
        (
            "subtask_uncheck",
            json!({"kind":"subtaskCheck","subtaskId":PROJECT_ID,"checked":false}),
        ),
        (
            "subtask_move",
            json!({"kind":"subtaskMove","subtaskId":PROJECT_ID,"place":"80"}),
        ),
        (
            "subtask_remove",
            json!({"kind":"subtaskRemove","subtaskId":PROJECT_ID}),
        ),
        (
            "work_trail_add",
            json!({"kind":"workTrailAdd","rowId":PROJECT_ID,"subject":null,"text":"trail","tags":[],"links":[]}),
        ),
        (
            "work_trail_redact",
            json!({"kind":"workTrailRedact","rowId":PROJECT_ID,"redacted":true}),
        ),
        (
            "role_grant",
            json!({"kind":"roleGrant","subject":subject,"role":"lead","holder":{"kind":"human","humanId":"schema-parity"},"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}),
        ),
        (
            "role_release",
            json!({"kind":"roleRelease","subject":subject,"role":"lead","holder":null,"expectedRevision":0}),
        ),
        (
            "owner_grant_record",
            json!({"kind":"ownerGrantRecord","grantId":PROJECT_ID,"grant":{"kind":"replaceLead","project":PROJECT_ID,"newHolder":actor,"reason":"replace"}}),
        ),
        (
            "role_replace_lead",
            json!({"kind":"roleReplaceLead","newHolder":actor,"ownerGrant":PROJECT_ID,"reason":"replace","expectedAssignmentRevision":0}),
        ),
        (
            "role_acknowledge_current",
            json!({"kind":"roleAcknowledgeCurrent","readThrough":cursor}),
        ),
        (
            "project_publish_plan",
            json!({"kind":"projectPublishPlan","hash":"a".repeat(64),"document":{"kind":"url","url":"https://example.com/plan"},"expectedRevision":0}),
        ),
        (
            "project_close",
            json!({"kind":"projectClose","reason":"complete","expectedRevision":0}),
        ),
        (
            "project_describe",
            json!({"kind":"projectDescribe","description":"metadata","expectedRevision":1}),
        ),
        (
            "fact_record",
            json!({"kind":"factRecord","fact":{"factId":PROJECT_ID,"kind":{"kind":"lifecycle","fact":{"kind":"closed","position":cursor,"reason":"complete"}},"links":[],"producerTime":"2026-10-18T08:00:00Z","anchor":null}}),
        ),
        (
            "",
            json!({"kind":"readMarkInitialize","reader":{"kind":"human","humanId":"schema-parity"},"scope":scope}),
        ),
        (
            "",
            json!({"kind":"readMarkAdvance","reader":{"kind":"human","humanId":"schema-parity"},"scope":scope,"through":cursor}),
        ),
        (
            "",
            json!({"kind":"threadCreate","postId":PROJECT_ID,"about":about,"text":"root","references":[],"participation":null,"watch":false,"addressees":[]}),
        ),
        (
            "",
            json!({"kind":"threadRefile","threadId":PROJECT_ID,"about":about}),
        ),
        (
            "",
            json!({"kind":"threadPost","postId":REQUEST_ID,"threadId":PROJECT_ID,"text":"reply","references":[],"addressees":[]}),
        ),
        (
            "",
            json!({"kind":"threadResolve","threadId":PROJECT_ID,"reason":"complete"}),
        ),
        ("", json!({"kind":"threadUnresolve","threadId":PROJECT_ID})),
        (
            "",
            json!({"kind":"threadJoin","threadId":PROJECT_ID,"role":"participant","watch":false,"mode":null,"whenIdle":null,"replace":null,"note":null}),
        ),
        (
            "",
            json!({"kind":"threadLeave","threadId":PROJECT_ID,"afterLeave":null}),
        ),
        ("", json!({"kind":"threadWatch","threadId":PROJECT_ID})),
        ("", json!({"kind":"threadUnwatch","threadId":PROJECT_ID})),
        ("", json!({"kind":"projectWatch"})),
        ("", json!({"kind":"projectUnwatch"})),
        (
            "",
            json!({"kind":"threadSubscribe","scope":scope,"policy":{}}),
        ),
        ("", json!({"kind":"threadUnsubscribe","scope":scope})),
        ("", json!({"kind":"threadRenew","scope":scope})),
    ]
}

#[tokio::test]
async fn all_project_schemas_match_real_handler_classification() {
    use crate::api_test_harness::{ServedApi, api_config, test_identity};
    use message_board::{ProjectRequest, ProjectTool};

    let cases = operation_cases();
    assert_eq!(cases.len(), 43);
    let kinds = cases
        .iter()
        .map(|(_, operation)| operation["kind"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(kinds.len(), 41);
    for (_, operation) in &cases {
        serde_json::from_value::<ProjectRequest>(request(operation.clone())).unwrap_or_else(
            |error| panic!("invalid {} parity fixture: {error}", operation["kind"]),
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let config = api_config((test_identity()).build(), directory.path());
    let api = ServedApi::tcp(&config).await;
    let (_, catalog) = api
        .post(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .await;
    let tools = catalog
        .pointer("/result/tools")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(ProjectTool::ALL.len(), 27);
    let mut accepted = 0;
    let mut refused = 0;
    for tool in ProjectTool::ALL {
        let advertised = tools
            .iter()
            .find(|advertised| advertised["name"] == tool.name())
            .unwrap();
        let validator = jsonschema::validator_for(&advertised["inputSchema"]).unwrap();
        for (expected_tool, operation) in &cases {
            let arguments = request(operation.clone());
            let expected = *expected_tool == tool.name();
            assert_eq!(
                validator.is_valid(&arguments),
                expected,
                "{} / {operation}",
                tool.name()
            );
            let (status, response) = api.post(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool.name(),"arguments":arguments}})).await;
            assert!(status.is_success(), "{}: {response}", tool.name());
            let failure = response.pointer("/result/structuredContent").unwrap();
            if expected {
                // A real application with no store proves the selector admitted the
                // input to the later admission gate, without inventing a write.
                assert_eq!(
                    failure["kind"],
                    "boardUnavailable",
                    "{}: {response}",
                    tool.name()
                );
                accepted += 1;
            } else {
                assert_eq!(
                    failure["kind"],
                    "invalidField",
                    "{}: {response}",
                    tool.name()
                );
                assert_eq!(failure.pointer("/details/field").unwrap(), "operation");
                refused += 1;
            }
        }
    }
    assert_eq!(accepted, 27);
    assert_eq!(refused, 1134);
    println!(
        "schema_handler_pairs={} selector_accepted={accepted} selector_refused={refused}",
        accepted + refused
    );
    api.stop().await;
}

#[tokio::test]
async fn real_tools_list_reports_input_and_output_bytes_separately() {
    use crate::api_test_harness::{ServedApi, api_config, test_identity};
    use message_board::ProjectTool;

    let directory = tempfile::tempdir().unwrap();
    let config = api_config((test_identity()).build(), directory.path());
    let api = ServedApi::tcp(&config).await;
    let (status, response) = api
        .post(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .await;
    assert!(status.is_success());
    let tools = response
        .pointer("/result/tools")
        .unwrap()
        .as_array()
        .unwrap();
    let mut input_bytes = 0;
    let mut output_bytes = 0;
    let mut project_input_bytes = 0;
    for tool in tools {
        let input_size = serde_json::to_vec(&tool["inputSchema"]).unwrap().len();
        input_bytes += input_size;
        output_bytes += serde_json::to_vec(&tool["outputSchema"]).unwrap().len();
        if ProjectTool::from_name(tool["name"].as_str().unwrap()).is_some() {
            project_input_bytes += input_size;
        }
        jsonschema::validator_for(&tool["inputSchema"]).expect("all local references resolve");
    }
    assert_eq!(tools.len(), 165);
    println!(
        "tools={} input_bytes={input_bytes} output_bytes={output_bytes} project_input_bytes={project_input_bytes} advisor_input_estimate=338678",
        tools.len()
    );
    api.stop().await;
}

#[test]
fn claim_schema_requires_attempt_and_project_without_an_actor_or_feed_cursor() {
    let server = CollaborationMcpServer::catalog_only();
    let tools = server.resolved_tools();
    let tool = tools
        .iter()
        .find(|tool| tool.name == "project_claim")
        .expect("claim tool");
    let input = Value::Object((*tool.input_schema).clone());
    let valid = json!({"requestId":REQUEST_ID,"protocolVersion":1,"projectId":PROJECT_ID});
    let validator = jsonschema::validator_for(&input).expect("claim input schema");
    assert!(validator.is_valid(&valid));
    let mut actor = valid;
    actor["actor"] = json!({"kind":"human","humanId":"cannot-authorize-claim"});
    assert!(!validator.is_valid(&actor));
    let output = Value::Object((**tool.output_schema.as_ref().expect("claim output")).clone());
    let validator = jsonschema::validator_for(&output).expect("claim output schema");
    let reply = super::super::domain_result(
        Ok::<_, message_board::BoardError>(board_sync_protocol::ProjectClaimReply::Claimed {
            epoch: fleet_sync_protocol::Epoch::new(2).expect("epoch"),
            expires_at: fleet_sync_protocol::SyncTime::from_unix_millis(1234),
            start: fleet_sync_protocol::ReplicationPosition::new(
                fleet_sync_protocol::Epoch::new(1).expect("epoch"),
                fleet_sync_protocol::CommitSequence::new(7),
            ),
        }),
        true,
    );
    let mut body = reply.structured_content.expect("claim response");
    assert!(validator.is_valid(&body));
    // Keep the actual native sequence. A board feed position has a different meaning.
    let text = body.to_string();
    assert!(text.contains("\"sequence\":7"));
    fn replace_sequence(value: &mut Value) {
        if let Some(fields) = value.as_object_mut() {
            if let Some(sequence) = fields.remove("sequence") {
                fields.insert("position".into(), sequence);
            }
            for value in fields.values_mut() {
                replace_sequence(value);
            }
        }
    }
    replace_sequence(&mut body);
    assert!(!validator.is_valid(&body));
}
