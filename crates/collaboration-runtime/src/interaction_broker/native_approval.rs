//! Legacy native approval broker integration.

use super::{
    APPROVAL_TIMEOUT, ApprovalBroker, ApprovalBrokerError, ApprovalRequestRecord, ApprovalRoute,
    ApprovalState, Arc, BrokeredApprovalOutcome, BrokeredApprovalRequest, CancellationMarker,
    NativeApprovalRefusalReason, PendingApproval, ServiceInteractionBroker, SessionRef, Value,
    map_native_options, native_approval_refusal_diagnostic, native_option_records, oneshot,
};

impl ApprovalBroker for ServiceInteractionBroker {
    fn register_route(
        &self,
        route: ApprovalRoute,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), ApprovalBrokerError>> + Send + '_>,
    > {
        Box::pin(async move {
            if route.created_by.endpoint.service_id != self.service_id
                || route.approver.endpoint.service_id != self.service_id
            {
                return Err(ApprovalBrokerError::RouteUnavailable);
            }
            let stored_route = interaction_storage::StoredApprovalRoute {
                thread_id: route.thread_id.clone(),
                route_json: serde_json::to_string(&route)
                    .map_err(|_| ApprovalBrokerError::Unavailable)?,
            };
            self.interaction_store
                .upsert_approval_route(&stored_route, chrono::Utc::now().timestamp_millis())
                .await
                .map_err(|_| ApprovalBrokerError::Unavailable)
        })
    }

    fn request(
        &self,
        request: BrokeredApprovalRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<BrokeredApprovalOutcome, ApprovalBrokerError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            let requester = SessionRef {
                endpoint: self.backend.endpoint.clone(),
                session_id: request
                    .thread_id
                    .clone()
                    .try_into()
                    .map_err(|_| ApprovalBrokerError::Unavailable)?,
            };
            let route = self.load_approval_route(&request.thread_id).await?;
            let Some(route) = route else {
                let diagnostic = native_approval_refusal_diagnostic(
                    &self.backend.endpoint,
                    &request.thread_id,
                    &request.request,
                    NativeApprovalRefusalReason::MissingRoute,
                );
                tracing::warn!(
                    endpoint = %diagnostic.endpoint,
                    provider_session_id = %diagnostic.provider_session_id,
                    method = diagnostic.method,
                    reason_code = diagnostic.reason_code,
                    "native approval request refused before route lookup",
                );
                return Err(ApprovalBrokerError::RouteUnavailable);
            };
            let native_options = request
                .request
                .pointer("/params/options")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if requester == route.approver {
                return Ok(BrokeredApprovalOutcome::Cancelled);
            }
            let offered = match map_native_options(native_options) {
                Ok(offered) => offered,
                Err(_) => return Ok(BrokeredApprovalOutcome::Cancelled),
            };
            let offered_options = native_option_records(native_options);
            let request_id = format!(
                "approval-{}",
                String::from(
                    crate::new_service_uuid().map_err(|_| ApprovalBrokerError::Unavailable)?
                )
            );
            let expires_at = (chrono::Utc::now()
                + chrono::Duration::seconds(APPROVAL_TIMEOUT.as_secs().cast_signed()))
            .to_rfc3339();
            let record = ApprovalRequestRecord {
                request_id: request_id.clone(),
                requester,
                approver: route.approver,
                generation: request.generation,
                state: ApprovalState::PendingClientDecision,
                reason: None,
                offered_options,
                presentation: None,
                operation: request.request,
                expires_at,
                decision: None,
            };
            let (completion, receiver) = oneshot::channel();
            self.pending.lock().await.insert(
                request_id.clone(),
                PendingApproval {
                    record: record.clone(),
                    offered,
                    completion,
                },
            );
            let mut cancellation = CancellationMarker {
                request_id: request_id.clone(),
                pending: Arc::clone(&self.pending),
                armed: true,
            };
            let deadline = tokio::time::Instant::now() + APPROVAL_TIMEOUT;
            let delivery = self.deliver(&record);
            tokio::pin!(delivery);
            let delivery_result = tokio::select! {
                biased;
                () = tokio::time::sleep_until(deadline) => {
                    self.remove_pending(&request_id).await;
                    cancellation.armed = false;
                    return Ok(BrokeredApprovalOutcome::Cancelled);
                }
                result = &mut delivery => result,
            };
            if delivery_result.is_err() {
                self.remove_pending(&request_id).await;
                cancellation.armed = false;
                return Ok(BrokeredApprovalOutcome::Cancelled);
            }
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(deadline) => {
                    self.remove_pending(&request_id).await;
                    cancellation.armed = false;
                    Ok(BrokeredApprovalOutcome::Cancelled)
                }
                result = receiver => match result {
                Ok(outcome) => {
                    cancellation.armed = false;
                    Ok(outcome)
                }
                Err(_) => {
                    cancellation.armed = false;
                    Ok(BrokeredApprovalOutcome::Cancelled)
                }
            }
            }
        })
    }

    fn route(
        &self,
        thread_id: &str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Option<ApprovalRoute>, ApprovalBrokerError>>
                + Send
                + '_,
        >,
    > {
        let thread_id = thread_id.to_owned();
        Box::pin(async move { self.load_approval_route(&thread_id).await })
    }
}
