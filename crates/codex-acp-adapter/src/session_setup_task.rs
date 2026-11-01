//! Native setup runs outside frontend routing; the registry publishes the returned binding.
use crate::{
    AcpSchemaCatalog, AcpSessionBinding, ConversationOperationRecorder, SessionSetupError,
    SessionSetupInputs,
};
use codex_native_integration::{NativePayloadSchemas, NativeProtocolConnection};
use collaboration_protocol::{CodexGeneration, OperationId, SessionId};
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

pub(crate) struct SetupTaskInputs {
    pub harness: crate::CodexHarnessAdmission,
    pub known_session: Option<AcpSessionBinding>,
    pub adopt_unmaterialized: bool,
    pub backend_path: PathBuf,
    pub schemas: Arc<NativePayloadSchemas>,
    pub generation: CodexGeneration,
    pub params: Value,
    pub create_new: bool,
    pub operation_id: Option<OperationId>,
    pub recorder: Arc<dyn ConversationOperationRecorder>,
    pub cancellation_barrier: Option<crate::CancellationBarrier>,
    pub approval_broker: Arc<dyn crate::ApprovalBroker>,
}
pub(crate) struct SetupTaskOutput {
    pub binding: Option<AcpSessionBinding>,
    pub outcome: Result<(Vec<Value>, Value), SessionSetupError>,
}
pub(crate) async fn run_session_setup(inputs: SetupTaskInputs) -> SetupTaskOutput {
    let managed = matches!(
        &inputs.harness,
        crate::CodexHarnessAdmission::MachineTokens { .. }
    );
    let mut catalog = match AcpSchemaCatalog::load() {
        Ok(catalog) => catalog,
        Err(_) => {
            return SetupTaskOutput {
                binding: inputs.known_session,
                outcome: Err(SessionSetupError::SchemaUnavailable),
            };
        }
    };
    if let Some(mut session) = inputs.known_session {
        let admission = match inputs.harness.restore(&inputs.params).await {
            Ok(admission) => admission,
            Err(error) => {
                return SetupTaskOutput {
                    binding: None,
                    outcome: Err(error),
                };
            }
        };
        if let Err(error) = session.require_harness_restore(&admission) {
            return SetupTaskOutput {
                binding: None,
                outcome: Err(error),
            };
        }
        if inputs.adopt_unmaterialized {
            let outcome = session
                .adopt_unmaterialized(&mut catalog, &inputs.generation, &inputs.params)
                .await
                .map(|()| (Vec::new(), session.setup_result()));
            return SetupTaskOutput {
                binding: Some(session),
                outcome,
            };
        }
        let outcome = match session
            .resume_with_receipt(&mut catalog, &inputs.generation, &inputs.params)
            .await
        {
            Ok(response)
                if crate::session_creation::native_thread_activity(&response)
                    == crate::session_creation::NativeThreadActivity::Active =>
            {
                Err(SessionSetupError::Busy)
            }
            Ok(response)
                if crate::session_creation::native_thread_activity(&response)
                    == crate::session_creation::NativeThreadActivity::Invalid =>
            {
                Err(SessionSetupError::NativeThreadStatusUnavailable)
            }
            Ok(response)
                if inputs
                    .cancellation_barrier
                    .as_ref()
                    .is_some_and(|barrier| !barrier.resolved_by_resume(&response)) =>
            {
                Err(SessionSetupError::CancellationUnresolved)
            }
            Ok(response) => {
                let history = if crate::history_projection::history_replay_requested(&inputs.params)
                {
                    crate::project_history(&mut catalog, session.session_id(), &response)
                } else {
                    Ok(Vec::new())
                };
                history
                    .map(|history| (history, session.setup_result()))
                    .map_err(|_| SessionSetupError::OutcomeUnknown)
            }
            Err(error) => Err(error),
        };
        let detached = matches!(
            &outcome,
            Err(SessionSetupError::Busy
                | SessionSetupError::NativeThreadStatusUnavailable
                | SessionSetupError::OutcomeUnknown
                | SessionSetupError::Unavailable)
        );
        return SetupTaskOutput {
            binding: if detached { None } else { Some(session) },
            outcome,
        };
    }
    let operation_id = inputs.operation_id.clone();
    if inputs.create_new {
        let Some(operation_id) = operation_id.as_ref() else {
            return SetupTaskOutput {
                binding: None,
                outcome: Err(SessionSetupError::InvalidParameters),
            };
        };
        if inputs
            .recorder
            .admit_create(operation_id, &inputs.generation)
            .await
            .is_err()
        {
            return SetupTaskOutput {
                binding: None,
                outcome: Err(SessionSetupError::Unavailable),
            };
        }
    }
    let outcome = async {
        let harness = if inputs.create_new {
            inputs
                .harness
                .prepare_start(
                    &mut catalog,
                    &inputs.params,
                    operation_id
                        .as_ref()
                        .ok_or(SessionSetupError::InvalidParameters)?,
                )
                .await?
        } else {
            inputs.harness.restore(&inputs.params).await?
        };
        let connection = match NativeProtocolConnection::connect(&inputs.backend_path).await {
            Ok(connection) => connection,
            Err(_) => {
                if let crate::HarnessSessionSetup::Start(prepared) = &harness {
                    prepared
                        .port
                        .creation_refused(&prepared.operation_id)
                        .await
                        .map_err(|_| SessionSetupError::OutcomeUnknown)?;
                }
                return Err(SessionSetupError::Unavailable);
            }
        };
        let setup = SessionSetupInputs {
            harness,
            connection,
            schemas: inputs.schemas,
            generation: inputs.generation,
            params: inputs.params,
            approval_broker: inputs.approval_broker,
            operation_id: operation_id.clone(),
            recorder: Arc::clone(&inputs.recorder),
        };
        if inputs.create_new {
            let session = AcpSessionBinding::create(&mut catalog, setup).await?;
            let session_id = SessionId::try_from(session.session_id().to_owned())
                .map_err(|_| SessionSetupError::OutcomeUnknown)?;
            if let Some(operation_id) = operation_id.as_ref() {
                inputs
                    .recorder
                    .record_created(operation_id, &session_id)
                    .await
                    .map_err(|_| SessionSetupError::OutcomeUnknown)?;
            }
            let result = session.new_session_result();
            Ok((session, Vec::new(), result))
        } else {
            let (session, history) = AcpSessionBinding::load_existing_guarded(
                &mut catalog,
                setup,
                inputs.cancellation_barrier.as_ref(),
            )
            .await?;
            let result = session.setup_result();
            Ok((session, history, result))
        }
    }
    .await;
    match outcome {
        Ok((session, history, result)) => SetupTaskOutput {
            binding: Some(session),
            outcome: Ok((history, result)),
        },
        Err(error) => {
            let known_not_submitted = if managed {
                matches!(
                    error,
                    SessionSetupError::LaunchAdmissionFailed(_) | SessionSetupError::NativeRejected
                )
            } else {
                matches!(
                    error,
                    SessionSetupError::InvalidParameters
                        | SessionSetupError::ConfigurationMismatch
                        | SessionSetupError::ModelMismatch { .. }
                        | SessionSetupError::AccessMismatch { .. }
                        | SessionSetupError::NativeRejected
                        | SessionSetupError::SchemaUnavailable
                        | SessionSetupError::HostLocationsUnavailable { .. }
                        | SessionSetupError::LaunchAdmissionFailed(_)
                )
            };
            let error = if let Some(operation_id) = operation_id.as_ref() {
                if inputs
                    .recorder
                    .record_failure(operation_id, known_not_submitted)
                    .await
                    .is_err()
                {
                    SessionSetupError::OutcomeUnknown
                } else {
                    error
                }
            } else {
                error
            };
            SetupTaskOutput {
                binding: None,
                outcome: Err(error),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SetupTaskInputs, run_session_setup};
    use crate::{
        AcpSchemaCatalog, AcpSessionBinding, ConversationOperationRecorder,
        ConversationRecordFuture, RejectingApprovalBroker, SessionSetupInputs,
    };
    use codex_native_integration::{
        NativePayloadSchemas, NativeProtocolConnection, NativeSchemaBundle,
    };
    use collaboration_protocol::{CodexGeneration, OperationId, SessionId};
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{Value, json};
    use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
    use tokio_tungstenite::{
        WebSocketStream,
        tungstenite::{Message, protocol::Role},
    };

    pub(super) struct AcceptingRecorder;

    impl ConversationOperationRecorder for AcceptingRecorder {
        fn admit_create<'a>(
            &'a self,
            _operation_id: &'a OperationId,
            _generation: &'a CodexGeneration,
        ) -> ConversationRecordFuture<'a> {
            Box::pin(async { Ok(()) })
        }

        fn before_native_dispatch<'a>(
            &'a self,
            _operation_id: &'a OperationId,
        ) -> ConversationRecordFuture<'a> {
            Box::pin(async { Ok(()) })
        }

        fn record_created<'a>(
            &'a self,
            _operation_id: &'a OperationId,
            _session_id: &'a SessionId,
        ) -> ConversationRecordFuture<'a> {
            Box::pin(async { Ok(()) })
        }

        fn record_failure<'a>(
            &'a self,
            _operation_id: &'a OperationId,
            _known_not_submitted: bool,
        ) -> ConversationRecordFuture<'a> {
            Box::pin(async { Ok(()) })
        }
    }

    pub(super) fn native_schemas() -> Arc<NativePayloadSchemas> {
        let definitions: serde_json::Map<String, Value> = [
            "ThreadFork",
            "ThreadLoadedList",
            "ThreadRead",
            "ThreadResume",
            "ThreadStart",
            "TurnInterrupt",
            "TurnStart",
            "TurnSteer",
        ]
        .into_iter()
        .flat_map(|name| {
            [
                (format!("{name}Params"), json!({"type":"object"})),
                (format!("{name}Response"), json!({"type":"object"})),
            ]
        })
        .collect();
        let bundle = NativeSchemaBundle::from_documents(BTreeMap::from([(
            "codex_app_server_protocol.schemas.json".to_owned(),
            serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))
                .unwrap_or_else(|error| panic!("JSON: {error}")),
        )]))
        .unwrap_or_else(|error| panic!("bundle: {error}"));
        Arc::new(
            NativePayloadSchemas::from_bundle(&bundle)
                .unwrap_or_else(|error| panic!("schemas: {error}")),
        )
    }

    #[tokio::test]
    async fn existing_binding_load_reports_permissions_and_skips_history_replay_capacity() {
        let schemas = native_schemas();
        let generation: CodexGeneration = serde_json::from_value(
            json!({"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1}),
        )
        .unwrap_or_else(|error| panic!("generation: {error}"));
        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        let connection = NativeProtocolConnection::from_websocket(
            WebSocketStream::from_raw_socket(client, Role::Client, None).await,
        );
        let long_items = (0..1025)
            .map(|index| {
                json!({"type":"agentMessage","id":format!("message-{index}"),"text":"old reply"})
            })
            .collect::<Vec<_>>();
        let fixture = tokio::spawn(async move {
            let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
            for items in [Vec::new(), long_items] {
                let frame = server.next().await.unwrap().unwrap();
                let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
                assert_eq!(request["method"], "thread/resume");
                assert_eq!(request["params"]["threadId"], "thread-a");
                server
                    .send(Message::Text(
                        json!({
                            "id":request["id"],
                            "result":{"cwd":"/work","thread":{"id":"thread-a","cwd":"/work","status":{"type":"idle"},"turns":[{"id":"turn-a","status":"completed","items":items}]}}
                        })
                        .to_string()
                        .into(),
                    ))
                    .await
                    .unwrap();
            }
        });

        let mut catalog =
            AcpSchemaCatalog::load().unwrap_or_else(|error| panic!("catalog: {error}"));
        let params = json!({
            "sessionId":"thread-a",
            "cwd":"/work",
            "mcpServers":[],
            "_meta":{"codex-router/replayHistory":false}
        });
        let (mut binding, initial_history) = AcpSessionBinding::load_existing(
            &mut catalog,
            SessionSetupInputs {
                harness: crate::HarnessSessionSetup::AccessEnrollment,
                operation_id: None,
                recorder: Arc::new(AcceptingRecorder),
                connection,
                schemas: Arc::clone(&schemas),
                generation: generation.clone(),
                params: params.clone(),
                approval_broker: Arc::new(RejectingApprovalBroker),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("initial load: {error}"));
        assert!(initial_history.is_empty());
        let actor: collaboration_protocol::SessionRef = serde_json::from_value(json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"creator"})).unwrap();
        // Fixture: this existing binding has a recorded Router profile; native omits it.
        binding.access_route = Some(crate::ApprovalRoute {
            thread_id: "thread-a".into(),
            created_by: actor.clone(),
            approver: actor,
            access: collaboration_protocol::RouterAccess::WorkspaceWrite,
            scratch_path: "/recorded-scratch".into(),
            root_message_id: None,
        });

        let resumed = run_session_setup(SetupTaskInputs {
            harness: crate::CodexHarnessAdmission::AccessEnrollment,
            known_session: Some(binding),
            adopt_unmaterialized: false,
            backend_path: PathBuf::new(),
            schemas: Arc::clone(&schemas),
            generation: generation.clone(),
            params: params.clone(),
            create_new: false,
            operation_id: None,
            recorder: Arc::new(AcceptingRecorder),
            cancellation_barrier: None,
            approval_broker: Arc::new(RejectingApprovalBroker),
        })
        .await;
        let (_history, result) = resumed
            .outcome
            .unwrap_or_else(|error| panic!("existing-binding resume: {error}"));
        let expected = json!(["profile", "sandbox", "writableRoots", "network"]);
        assert_eq!(
            result["_meta"]["codexRouter"]["permissionDifferences"],
            expected
        );
        let mut binding = resumed.binding.unwrap();
        binding.materialized = false;
        let adopted = run_session_setup(SetupTaskInputs {
            harness: crate::CodexHarnessAdmission::AccessEnrollment,
            known_session: Some(binding),
            adopt_unmaterialized: true,
            backend_path: PathBuf::new(),
            schemas,
            generation,
            params,
            create_new: false,
            operation_id: None,
            recorder: Arc::new(AcceptingRecorder),
            cancellation_barrier: None,
            approval_broker: Arc::new(RejectingApprovalBroker),
        })
        .await;
        let (_, result) = adopted.outcome.unwrap();
        assert_eq!(
            result["_meta"]["codexRouter"]["permissionDifferences"],
            expected
        );
        assert!(adopted.binding.is_some());
        fixture.await.unwrap();
    }
}

#[cfg(test)]
#[path = "session_setup_task_harness_tests.rs"]
mod harness_tests;
