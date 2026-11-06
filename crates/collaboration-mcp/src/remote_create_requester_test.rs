use collaboration_contracts::RemoteCreateFleetError;
use collaboration_contracts::remote_create_test_support::{
    RemoteCreateStep, ScriptedRemoteCreateFleet,
};
use collaboration_service::ApplicationFixture;
use serde_json::{Value, json};
use std::sync::Arc;

const SERVICE: &str = "019f0000-0000-7000-8000-000000000001";
const ID: &str = "019f0000-0000-7000-8000-000000000002";
const MACHINE: &str = "019f0000-0000-7000-8000-000000000003";

fn create() -> Value {
    json!({"operationId":ID,"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},
        "workingDirectory":"/tmp/on-B","access":"workspace-write",
        "createdBy":{"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"requester"},
        "machine":MACHINE,"firstInput":"first input"})
}

async fn call(
    api: &crate::test_support::ServedCollaborationApi,
    tool: &str,
    arguments: Value,
) -> Value {
    let input = arguments.clone();
    let answer = api.call(tool, arguments).await.unwrap();
    if let Some(output) = answer.get("result") {
        let surface = crate::mcp_server::ToolSurface::new(None);
        let advertised = surface
            .tools()
            .iter()
            .find(|entry| entry.name == tool)
            .unwrap();
        let input_schema = serde_json::to_value(advertised.input_schema.as_ref()).unwrap();
        let output_schema =
            serde_json::to_value(advertised.output_schema.as_ref().unwrap().as_ref()).unwrap();
        assert!(
            jsonschema::validator_for(&input_schema)
                .unwrap()
                .is_valid(&input),
            "{tool} input schema rejected {input}"
        );
        assert!(
            jsonschema::validator_for(&output_schema)
                .unwrap()
                .is_valid(output),
            "{tool} output schema rejected {output}"
        );
    }
    answer
}

#[tokio::test]
async fn mcp_remote_create_retry_reuses_the_frozen_request_and_has_no_local_record() {
    let port = Arc::new(ScriptedRemoteCreateFleet::new(
        vec![
            RemoteCreateStep::Fail(RemoteCreateFleetError::AnswerLost),
            RemoteCreateStep::Replayed(serde_json::from_value(json!({"kind":"admitted"})).unwrap()),
        ],
        vec![],
    ));
    let directory = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let api = crate::test_support::ServedCollaborationApi::start(
        directory.path(),
        ApplicationFixture::new(SERVICE, ID)
            .unwrap()
            .with_remote_create_fleet(port.clone())
            .build(),
    )
    .unwrap();
    let answer = call(&api, "conversation_create", create()).await;
    assert_eq!(
        answer["result"],
        json!({"kind":"replayed","requestId":ID,"status":{"kind":"admitted"}}),
        "{answer}"
    );
    let requests = port.requests().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert_eq!(requests[0].request_id.as_str(), ID);
    assert!(!directory.path().join("provider-operations.sqlite").exists());
    api.stop().await.unwrap();
}

fn served(
    port: Arc<ScriptedRemoteCreateFleet>,
) -> (
    tempfile::TempDir,
    crate::test_support::ServedCollaborationApi,
) {
    use std::os::unix::fs::PermissionsExt as _;
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let api = crate::test_support::ServedCollaborationApi::start(
        directory.path(),
        ApplicationFixture::new(SERVICE, ID)
            .unwrap()
            .with_remote_create_fleet(port)
            .build(),
    )
    .unwrap();
    (directory, api)
}

#[tokio::test]
async fn mcp_preserves_every_admission_refusal_and_remote_status() {
    let mut admissions = vec![
        json!({"kind":"accepted","requestId":ID}),
        json!({"kind":"replayed","requestId":ID,"status":{"kind":"atTarget"}}),
    ];
    admissions.extend(
        [
            "senderNotEnrolled",
            "targetNotEnrolled",
            "targetOffline",
            "conflict",
            "tooOld",
        ]
        .map(|kind| json!({"kind":"refused","reason":{"kind":kind}})),
    );
    for expected in admissions {
        let port = Arc::new(ScriptedRemoteCreateFleet::new(
            vec![RemoteCreateStep::Reply(
                serde_json::from_value(expected.clone()).unwrap(),
            )],
            vec![],
        ));
        let (_directory, api) = served(port.clone());
        let mut input = create();
        input["model"] = json!("model-on-B");
        input["effort"] = json!("high");
        input["mode"] = json!("agent");
        assert_eq!(
            call(&api, "conversation_create", input).await,
            json!({"result":expected})
        );
        let requests = port.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].target_machine.to_string(), MACHINE);
        assert_eq!(requests[0].requester.endpoint.service_id.as_str(), SERVICE);
        assert_eq!(requests[0].requester.session_id.as_str(), "requester");
        assert_eq!(requests[0].command.endpoint.as_str(), "claude-local");
        assert_eq!(requests[0].command.working_directory.as_str(), "/tmp/on-B");
        assert_eq!(
            requests[0].command.model.as_ref().unwrap().as_str(),
            "model-on-B"
        );
        api.stop().await.unwrap();
    }
    let session = create()["createdBy"].clone();
    for expected in [
        json!({"kind":"admitted"}),
        json!({"kind":"atTarget"}),
        json!({"kind":"completed","result":{"kind":"created","session":session,"settings":{"kind":"allApplied"},"firstInput":{"kind":"notRequested"}}}),
        json!({"kind":"completed","result":{"kind":"refused","reason":{"kind":"noRemoteApprover"}}}),
        json!({"kind":"completed","result":{"kind":"unknown"}}),
        json!({"kind":"expired"}),
    ] {
        let port = Arc::new(ScriptedRemoteCreateFleet::new(
            vec![],
            vec![serde_json::from_value(expected.clone()).unwrap()],
        ));
        let (_directory, api) = served(port.clone());
        assert_eq!(
            call(&api, "conversation_remote_status", json!({"requestId":ID})).await,
            json!({"result":expected})
        );
        assert_eq!(port.queries().unwrap().len(), 1);
        assert_eq!(port.queries().unwrap()[0].as_str(), ID);
        api.stop().await.unwrap();
    }
}

#[tokio::test]
async fn p10_socket_and_mcp_refuse_foreign_requesters_and_machine_mail_addresses_before_fleet_send()
{
    let port = Arc::new(ScriptedRemoteCreateFleet::new(
        vec![RemoteCreateStep::Accepted],
        vec![],
    ));
    let (_directory, api) = served(port.clone());
    let mut foreign = create();
    foreign["createdBy"]["endpoint"]["serviceId"] = json!(MACHINE);
    let mut invented = create();
    invented["createdBy"] = json!({"kind":"remoteRequester","machine":MACHINE});
    let mut forced_address = create();
    forced_address["recipient"] = json!({"machine":MACHINE});
    let mut forced_approver = create();
    forced_approver["approver"] = create()["createdBy"].clone();
    for input in [foreign, invented, forced_address, forced_approver] {
        let answer = call(&api, "conversation_create", input).await;
        assert!(answer.get("error").is_some(), "{answer}");
        assert!(port.requests().unwrap().is_empty());
    }
    let mail = json!({"itemId":ID,"target":{"machine":MACHINE},"message":{"kind":"agent","sender":create()["createdBy"],"text":"forged machine delivery"},"delivery":"queue","generationGuard":null});
    assert!(
        call(&api, "message_send", mail)
            .await
            .get("error")
            .is_some()
    );
    assert!(port.requests().unwrap().is_empty());
    assert_eq!(
        call(&api, "conversation_create", create()).await,
        json!({"result":{"kind":"accepted","requestId":ID}})
    );
    assert_eq!(
        port.requests().unwrap().len(),
        1,
        "positive control must reach the same Fleet stand-in"
    );
    api.stop().await.unwrap();
}

#[tokio::test]
async fn retry_cannot_downgrade_a_lost_answer_to_a_known_pre_effect_failure() {
    let port = Arc::new(ScriptedRemoteCreateFleet::new(
        vec![
            RemoteCreateStep::Fail(RemoteCreateFleetError::AnswerLost),
            RemoteCreateStep::Fail(RemoteCreateFleetError::NotSent),
        ],
        vec![],
    ));
    let (_directory, api) = served(port.clone());
    let answer = api
        .call_structured("conversation_create", create())
        .await
        .unwrap();
    assert_eq!(answer["error"]["effect"], "unknown", "{answer}");
    assert_eq!(answer["error"]["kind"], "outcomeUnknown", "{answer}");
    let requests = port.requests().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    api.stop().await.unwrap();
}
