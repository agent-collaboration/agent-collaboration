//! Remote creates select a peer without probing a target-local carrier on A.
use super::{ConversationAccess, CreateArguments, RemoteStatusArguments, current_session_ref};
use collaboration_client::protocol::{EndpointRef, OperationId, RouterAccess};
use collaboration_client::{
    CollaborationClient, ConversationClient, ConversationCreateActor, ConversationCreateInput,
    FirstInput, RemoteCreateAdmission,
};
use std::{
    io::{self, Read, Write},
    time::Duration,
};

impl CreateArguments {
    pub(super) fn run_remote(self) -> i32 {
        let json = self.json;
        let result = (|| -> Result<_, String> {
            if self.approver.is_some()
                || self.approver_owner
                || self.fork.is_some()
                || self.generation.is_some()
                || self.root_message_id.is_some()
            {
                return Err("--machine cannot set --approver, --approver-owner, --fork, --generation or --root-message-id".into());
            }
            let machine: message_board::RecordId = self
                .machine
                .as_deref()
                .ok_or("--machine is required")?
                .to_owned()
                .try_into()
                .map_err(|_| "--machine must be a UUIDv7 Fleet peer")?;
            let operation = self
                .operation_id
                .clone()
                .map(TryInto::try_into)
                .transpose()
                .map_err(|_| "--operation-id must be UUIDv7")?
                .unwrap_or_else(OperationId::generate);
            let text = match (&self.text, &self.text_file) {
                (Some(text), None) => Some(text.clone()),
                (None, Some(path)) if path.as_os_str() == "-" => {
                    let mut text = String::new();
                    io::stdin()
                        .read_to_string(&mut text)
                        .map_err(|_| "could not read --text-file stdin")?;
                    Some(text)
                }
                (None, Some(path)) => {
                    Some(std::fs::read_to_string(path).map_err(|_| "could not read --text-file")?)
                }
                (None, None) => None,
                _ => return Err("--text conflicts with --text-file".into()),
            };
            let first_input = text
                .map(FirstInput::try_from)
                .transpose()
                .map_err(|_| "first input must satisfy the native 1 MiB text contract")?;
            let directory =
                crate::endpoint_commands::resolve_directory(self.service_directory.clone())?;
            Ok((machine, operation, first_input, directory))
        })();
        let (machine, operation_id, first_input, directory) = match result {
            Ok(value) => value,
            Err(error) => {
                return crate::endpoint_commands::report_failure("invalidField", &error, 2, json);
            }
        };
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => return 3,
        };
        runtime.block_on(async move {
            let client = match CollaborationClient::connect(
                &directory,
                "agent-collaboration",
                env!("CARGO_PKG_VERSION"),
            )
            .await
            {
                Ok(client) => client,
                Err(error) => {
                    return crate::endpoint_commands::report_failure(
                        "unavailable",
                        &error.to_string(),
                        3,
                        json,
                    );
                }
            };
            let endpoint: EndpointRef = if self.endpoint.trim_start().starts_with('{') {
                match serde_json::from_str(&self.endpoint) {
                    Ok(endpoint) => endpoint,
                    Err(_) => {
                        return crate::endpoint_commands::report_failure(
                            "invalidField",
                            "invalid --endpoint",
                            2,
                            json,
                        );
                    }
                }
            } else {
                let endpoint_id = match self.endpoint.try_into() {
                    Ok(id) => id,
                    Err(_) => {
                        return crate::endpoint_commands::report_failure(
                            "invalidField",
                            "invalid --endpoint",
                            2,
                            json,
                        );
                    }
                };
                EndpointRef {
                    service_id: client.identity().service_id.clone(),
                    endpoint_id,
                }
            };
            let created_by: ConversationCreateActor = match self.from {
                Some(value) => match serde_json::from_str(&value) {
                    Ok(actor) => actor,
                    Err(_) => {
                        return crate::endpoint_commands::report_failure(
                            "invalidField",
                            "invalid --from SessionRef",
                            2,
                            json,
                        );
                    }
                },
                None => match current_session_ref(&client.identity().service_id, None) {
                    Ok(session) => session.into(),
                    Err(_) => {
                        return crate::endpoint_commands::report_failure(
                            "invalidField",
                            "current requester Session unavailable; pass --from",
                            2,
                            json,
                        );
                    }
                },
            };
            let input = ConversationCreateInput {
                operation_id,
                endpoint,
                created_by,
                working_directory: self.cwd,
                access: match self.access {
                    ConversationAccess::WriteRestricted => RouterAccess::WriteRestricted,
                    ConversationAccess::WorkspaceWrite => RouterAccess::WorkspaceWrite,
                },
                model: self.model,
                effort: self.effort,
                mode: self.mode,
                approver: None,
                generation: None,
                fork: None,
                root_message_id: None,
            };
            let frozen = match ConversationClient::remote_create_request(
                &input,
                &machine,
                first_input.clone(),
                &client.identity().service_id,
            ) {
                Ok(request) => request,
                Err(error) => {
                    return crate::endpoint_commands::report_failure(
                        "invalidField",
                        &error.to_string(),
                        2,
                        json,
                    );
                }
            };
            if json {
                let _ = writeln!(
                    io::stdout(),
                    "{}",
                    serde_json::json!({
                        "kind": "remoteCreateRequested", "requestId": frozen.request_id,
                    })
                );
            } else {
                let _ = writeln!(io::stdout(), "request id: {}", frozen.request_id.as_str());
            }
            let operation_id = input.operation_id.clone();
            match ConversationClient::create_remote(
                &client,
                input,
                machine,
                first_input,
                Duration::from_secs(self.timeout_seconds),
            )
            .await
            {
                Ok(answer) => {
                    let exit = if matches!(answer, RemoteCreateAdmission::Refused { .. }) {
                        2
                    } else {
                        0
                    };
                    match serde_json::to_string(&answer) {
                        Ok(value) => {
                            let _ = writeln!(io::stdout(), "{value}");
                            exit
                        }
                        Err(_) => 3,
                    }
                }
                Err(error) => {
                    super::client_commands::report_remote_create_failure(error, &operation_id, json)
                }
            }
        })
    }
}

impl RemoteStatusArguments {
    pub(super) fn run(self) -> i32 {
        let request = match self.request_id.try_into() {
            Ok(request) => request,
            Err(_) => {
                return crate::endpoint_commands::report_failure(
                    "invalidField",
                    "--request-id must be UUIDv7",
                    2,
                    self.json,
                );
            }
        };
        let directory = match crate::endpoint_commands::resolve_directory(self.service_directory) {
            Ok(directory) => directory,
            Err(error) => {
                return crate::endpoint_commands::report_failure(
                    "invalidField",
                    &error,
                    2,
                    self.json,
                );
            }
        };
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => return 3,
        };
        runtime.block_on(async move {
            let client = match CollaborationClient::connect(
                &directory,
                "agent-collaboration",
                env!("CARGO_PKG_VERSION"),
            )
            .await
            {
                Ok(client) => client,
                Err(error) => {
                    return crate::endpoint_commands::report_failure(
                        "unavailable",
                        &error.to_string(),
                        3,
                        self.json,
                    );
                }
            };
            match ConversationClient::remote_status(
                &client,
                request,
                Duration::from_secs(self.timeout_seconds),
            )
            .await
            {
                Ok(status) => match serde_json::to_string(&status) {
                    Ok(value) => {
                        let _ = writeln!(io::stdout(), "{value}");
                        0
                    }
                    Err(_) => 3,
                },
                Err(error) => crate::endpoint_commands::report_failure(
                    "unavailable",
                    &error.to_string(),
                    3,
                    self.json,
                ),
            }
        })
    }
}
