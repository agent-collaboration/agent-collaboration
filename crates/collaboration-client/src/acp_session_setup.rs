//! ACP session selection and its setup evidence.
use super::{
    AcpConversation, ClientError, ConversationCreateRequest, ConversationCreateResult,
    ConversationEvent, SessionRef, SetupUpdateDelivery, Value, create_private_scratch, json,
    prepare_project_write_areas, router_metadata_effort, session_scratch_scope,
    validate_conversation_create_request, validate_conversation_endpoint,
};

impl AcpConversation {
    /// Connects, initializes, and selects exactly one existing or newly-created
    /// Codex conversation. The returned client remains attached for a following
    /// prompt; dropping it never deletes the backend conversation.
    pub async fn create(
        access: &crate::CollaborationAccess,
        request: ConversationCreateRequest,
        emit: &mut impl FnMut(ConversationEvent) -> Result<(), ClientError>,
    ) -> Result<(Self, ConversationCreateResult), crate::OperationError> {
        validate_conversation_create_request(&request)
            .map_err(|source| crate::OperationError::before_dispatch("validation", None, source))?;
        let endpoint_id = request.endpoint.endpoint_id.clone();
        let mut client = Self::connect_with_context(access, endpoint_id).await?;
        validate_conversation_endpoint(&client.endpoint, &request.endpoint)
            .map_err(|source| crate::OperationError::before_dispatch("validation", None, source))?;
        let target = client
            .open_session_with_context(&request, emit, SetupUpdateDelivery::StreamToCaller)
            .await?;
        let permission_differences = client.permission_differences.clone();
        Ok((
            client,
            ConversationCreateResult {
                target,
                permission_differences,
            },
        ))
    }
    pub async fn open_session(
        &mut self,
        request: &ConversationCreateRequest,
        emit: &mut impl FnMut(ConversationEvent) -> Result<(), ClientError>,
    ) -> Result<SessionRef, ClientError> {
        self.open_session_with_context(request, emit, SetupUpdateDelivery::StreamToCaller)
            .await
            .map_err(crate::OperationError::into_source)
    }

    pub(crate) async fn open_session_with_context(
        &mut self,
        request: &ConversationCreateRequest,
        emit: &mut impl FnMut(ConversationEvent) -> Result<(), ClientError>,
        setup_updates: SetupUpdateDelivery,
    ) -> Result<SessionRef, crate::OperationError> {
        validate_conversation_create_request(request)
            .map_err(|source| crate::OperationError::before_dispatch("validation", None, source))?;
        // A requested switch retires the previous selection even when setup rejects
        // or its future is cancelled. Only completed setup may enable a prompt.
        self.session_ready = false;
        self.target = None;
        self.pending_updates.clear();
        self.pending_bytes = 0;
        self.permission_differences.clear();
        if !request.cwd.is_absolute() {
            return Err(crate::OperationError::before_dispatch(
                "validation",
                None,
                ClientError::Protocol("ACP cwd must be absolute"),
            ));
        }
        let target = if let Some(id) = &request.session {
            let target = SessionRef {
                endpoint: self.endpoint.clone(),
                session_id: id.clone(),
            };
            if !self.load_supported {
                return Err(crate::OperationError::before_dispatch(
                    "load",
                    Some(target),
                    ClientError::UnsupportedCapability("ACP session/load"),
                ));
            }
            self.target = Some(target.clone());
            let mut metadata = serde_json::Map::new();
            metadata.insert(
                "codexRouter".to_owned(),
                router_metadata_effort(request.effort.as_deref()),
            );
            metadata.insert("router".to_owned(), json!({"sessionRef":target}));
            if setup_updates == SetupUpdateDelivery::Discard {
                metadata.insert("codex-router/replayHistory".to_owned(), json!(false));
            }
            let params = json!({"sessionId":id,"cwd":request.cwd,"mcpServers":[],"_meta":metadata});
            self.validate("LoadSessionRequest", &params)
                .map_err(|source| {
                    crate::OperationError::before_dispatch("load", Some(target.clone()), source)
                })?;
            let result = self
                .request(
                    "session/load",
                    params,
                    setup_updates,
                    "LoadSessionRequest",
                    "LoadSessionResponse",
                )
                .await
                .map_err(|source| {
                    crate::OperationError::after_dispatch(
                        "load",
                        Some(target.clone()),
                        None,
                        source,
                    )
                })?;
            self.permission_differences = permission_differences(&result).map_err(|source| {
                crate::OperationError::after_dispatch("load", Some(target.clone()), None, source)
            })?;
            target
        } else {
            // An absent model or effort is omitted, not sent as null: on fork the
            // adapter reads the source thread to fill it.
            let mut router_metadata = serde_json::Map::new();
            if let Some(model) = &request.model {
                router_metadata.insert("model".to_owned(), json!(model));
            }
            if let Some(effort) = &request.effort {
                router_metadata.insert("effort".to_owned(), json!(effort));
            }
            router_metadata.extend([
                ("access".to_owned(), json!(request.access)),
                ("createdBy".to_owned(), json!(request.created_by)),
                ("approver".to_owned(), json!(request.approver)),
            ]);
            let scratch_scope = request
                .root_message_id
                .as_ref()
                .map(|value| String::from(value.clone()))
                .unwrap_or_else(session_scratch_scope);
            let scratch_path = self
                .service_directory
                .parent()
                .ok_or(ClientError::Protocol("service directory has no owner root"))
                .map_err(|source| {
                    crate::OperationError::before_dispatch("preparation", None, source)
                })?
                .join("scratch")
                .join(&scratch_scope);
            prepare_project_write_areas(&request.cwd, request.access.as_deref())
                .map_err(ClientError::from)
                .map_err(|source| {
                    crate::OperationError::before_dispatch("preparation", None, source)
                })?;
            create_private_scratch(&scratch_path).map_err(|source| {
                crate::OperationError::before_dispatch("preparation", None, source)
            })?;
            router_metadata.insert("rootMessageId".into(), json!(request.root_message_id));
            router_metadata.insert("scratchScope".into(), json!(scratch_scope));
            router_metadata.insert("scratchPath".into(), json!(scratch_path));
            router_metadata.insert("operationId".into(), json!(request.operation_id));
            if let Some(source_thread_id) = &request.fork {
                router_metadata.insert("forkThreadId".into(), json!(source_thread_id));
            }
            let params =
                json!({"cwd":request.cwd,"mcpServers":[],"_meta":{"codexRouter":router_metadata}});
            let stage = if request.fork.is_some() {
                "fork"
            } else {
                "new"
            };
            self.validate("NewSessionRequest", &params)
                .map_err(|source| crate::OperationError::before_dispatch(stage, None, source))?;
            let result = self
                .request(
                    "session/new",
                    params,
                    setup_updates,
                    "NewSessionRequest",
                    "NewSessionResponse",
                )
                .await
                .map_err(|source| {
                    crate::OperationError::after_dispatch(stage, None, None, source)
                })?;
            self.permission_differences = permission_differences(&result).map_err(|source| {
                crate::OperationError::after_dispatch(stage, None, None, source)
            })?;
            SessionRef {
                endpoint: self.endpoint.clone(),
                session_id: result
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .ok_or(ClientError::Protocol("ACP session ID missing"))
                    .map_err(|source| {
                        crate::OperationError::after_dispatch(stage, None, None, source)
                    })?
                    .to_owned()
                    .try_into()
                    .map_err(|_| {
                        crate::OperationError::after_dispatch(
                            stage,
                            None,
                            None,
                            ClientError::Protocol("invalid session ID"),
                        )
                    })?,
            }
        };
        self.target = Some(target.clone());
        emit(ConversationEvent::SessionReady(target.clone())).map_err(|source| {
            crate::OperationError::after_dispatch("setup", Some(target.clone()), None, source)
        })?;
        for update in std::mem::take(&mut self.pending_updates) {
            emit(ConversationEvent::SessionUpdate {
                target: target.clone(),
                update,
            })
            .map_err(|source| {
                crate::OperationError::after_dispatch("setup", Some(target.clone()), None, source)
            })?;
        }
        self.pending_bytes = 0;
        self.session_ready = true;
        Ok(target)
    }
}

fn permission_differences(
    result: &Value,
) -> Result<Vec<collaboration_protocol::PermissionDifference>, ClientError> {
    result
        .pointer("/_meta/codexRouter/permissionDifferences")
        .map_or_else(
            || Ok(Vec::new()),
            |differences| {
                serde_json::from_value(differences.clone())
                    .map_err(|_| ClientError::Protocol("invalid permission differences"))
            },
        )
}

#[cfg(test)]
mod tests {
    use super::permission_differences;
    use collaboration_protocol::PermissionDifference;
    use serde_json::json;

    #[test]
    fn setup_reports_parse_closed_classes_and_accept_old_responses() {
        assert_eq!(permission_differences(&json!({})).unwrap(), vec![]);
        assert_eq!(
            permission_differences(
                &json!({"_meta":{"codexRouter":{"permissionDifferences":["writableRoots"]}}})
            )
            .unwrap(),
            vec![PermissionDifference::WritableRoots]
        );
        for invalid in [
            json!(["/private/path"]),
            json!(["unknown"]),
            json!("writableRoots"),
            json!(null),
        ] {
            assert!(
                permission_differences(
                    &json!({"_meta":{"codexRouter":{"permissionDifferences":invalid}}})
                )
                .is_err()
            );
        }
    }
}
