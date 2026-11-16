use super::*;

#[cfg(unix)]
#[tokio::test]
async fn supervisor_shutdown_joins_runtime_and_settles_held_work() -> TestResult {
    let root = tempfile::tempdir()?;
    let provider_endpoint = endpoint("claude-local")?;
    let runtime = ExternalProviderRuntime::initialize(
        cancel_fixture(&root.path().join("shutdown.log")),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await?;
    let backend = supervisor(
        &root,
        vec![(binding(provider_endpoint.clone(), "claude")?, runtime)],
    )
    .await?;
    let requester = actor(provider_endpoint.clone(), "requester")?;
    let create_id = OperationId::generate();
    operation(
        backend
            .create(ConversationCreateRequest {
                settings: Some(collaboration_protocol::ProviderRequestedSettings {
                    mode: Some("auto".into()),
                    ..collaboration_protocol::ProviderRequestedSettings::default()
                }),
                operation_id: create_id.clone(),
                endpoint: provider_endpoint,
                generation: Some(generation()?),
                working_directory: working_directory()?,
                created_by: (requester.clone()).into(),
                approver: (requester.clone()).into(),
                requested_policy: policy(),
            })
            .await,
    )?;
    let created = operation(wait(&backend, create_id).await)?;
    let target = match created.output {
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::Created { target, .. },
        } => target,
        other => return Err(format!("unexpected create output: {other:?}").into()),
    };
    let prompt_id = OperationId::generate();
    operation(
        backend
            .prompt(ConversationPromptRequest {
                delivery: collaboration_protocol::MessageDelivery::Auto,
                input_id: None,
                operation_id: prompt_id.clone(),
                target,
                generation: Some(generation()?),
                requested_by: (requester.clone()).into(),
                approver: (requester).into(),
                prompt: MessageContent::HumanUser {
                    text: MessageText::try_from("hold".to_owned())?,
                },
            })
            .await,
    )?;
    await_admission(&backend, &prompt_id).await?;
    backend
        .shutdown()
        .await
        .map_err(|message| message.to_owned())?;
    let failure = wait(&backend, prompt_id)
        .await
        .expect_err("shutdown-held prompt cannot succeed");
    ensure_eq!(failure.effect, ProviderOperationEffect::Unknown);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn supervisor_shutdown_drains_saturated_load_completions_and_preserves_unknown_effect()
-> TestResult {
    let root = tempfile::tempdir()?;
    let admission_log = root.path().join("load-admissions.log");
    let process_id_path = root.path().join("provider.pid");
    let provider_endpoint = endpoint("claude-local")?;
    let runtime = ExternalProviderRuntime::initialize(
        saturated_load_fixture(&admission_log, &process_id_path),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await?;
    let backend = supervisor(
        &root,
        vec![(binding(provider_endpoint.clone(), "claude")?, runtime)],
    )
    .await?;
    let requester = actor(provider_endpoint.clone(), "requester")?;
    let mut operation_ids = Vec::new();
    for index in 0..40 {
        let operation_id = OperationId::generate();
        operation(
            backend
                .load(ConversationLoadRequest {
                    operation_id: operation_id.clone(),
                    target: actor(provider_endpoint.clone(), &format!("held-load-{index}"))?,
                    generation: Some(generation()?),
                    working_directory: working_directory()?,
                    requested_by: (requester.clone()).into(),
                    approver: (requester.clone()).into(),
                    requested_policy: policy(),
                })
                .await,
        )?;
        operation_ids.push(operation_id);
    }
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if std::fs::read_to_string(&admission_log)
                .unwrap_or_default()
                .lines()
                .count()
                == 40
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    backend
        .shutdown()
        .await
        .map_err(|message| message.to_owned())?;
    for operation_id in operation_ids {
        let failure = wait(&backend, operation_id)
            .await
            .expect_err("shutdown-held load cannot succeed");
        ensure_eq!(failure.effect, ProviderOperationEffect::Unknown);
    }
    assert_process_reaped(&process_id_path).await?;
    Ok(())
}
