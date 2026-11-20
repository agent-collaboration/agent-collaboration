//! One prompt actor keeps frontend cancellation responsive while native work is pending.
use crate::{AcpSchemaCatalog, AcpSessionBinding, PendingAcpPrompt, PromptEvent};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub enum PromptCommand {
    Cancel,
}
pub struct PromptTaskInputs {
    pub session: AcpSessionBinding,
    pub request_id: Value,
    pub params: Value,
    pub commands: mpsc::Receiver<PromptCommand>,
    pub output: crate::AcpOutputSender,
    pub retired: CancellationToken,
}
/// The registry installs this binding before publishing the terminal response.
#[derive(Default)]
pub struct PromptTaskCompletion {
    pub binding: Option<AcpSessionBinding>,
    pub terminal: Option<Value>,
    pub cancellation_barrier: Option<crate::CancellationBarrier>,
}
pub async fn run_prompt_task(mut inputs: PromptTaskInputs) -> PromptTaskCompletion {
    if inputs.retired.is_cancelled() {
        return PromptTaskCompletion::default();
    }
    if let Ok(command) = inputs.commands.try_recv() {
        let terminal = if matches!(command, PromptCommand::Cancel) {
            cancelled_response(&inputs.request_id, "notDispatched")
        } else {
            failure(
                &inputs.request_id,
                "Unexpected permission response before prompt dispatch",
            )
        };
        return PromptTaskCompletion {
            cancellation_barrier: None,
            binding: Some(inputs.session),
            terminal: Some(terminal),
        };
    }
    let mut catalog = match AcpSchemaCatalog::load() {
        Ok(catalog) => catalog,
        Err(_error) => {
            return PromptTaskCompletion {
                cancellation_barrier: None,
                binding: Some(inputs.session),
                terminal: Some(failure(&inputs.request_id, "Native schema unavailable")),
            };
        }
    };
    let mut commands_open = true;
    let mut frontend_detached = false;
    let pending = {
        let start = PendingAcpPrompt::start(
            inputs.session,
            &mut catalog,
            inputs.request_id.clone(),
            &inputs.params,
        );
        tokio::pin!(start);
        tokio::select! {
            biased;
            _=inputs.retired.cancelled()=>return PromptTaskCompletion::default(),
            command=inputs.commands.recv(),if commands_open=>{
                let terminal=match command {
                    Some(PromptCommand::Cancel)=>Some(cancelled_response(&inputs.request_id,"unknown")),
                    None=>{
                        commands_open=false;
                        frontend_detached=true;
                        inputs.commands.close();
                        None
                    },
                };
                if commands_open || terminal.is_some() {
                    return PromptTaskCompletion {cancellation_barrier:Some(crate::CancellationBarrier::UnknownTurn),binding:None,terminal};
                }
                start.await
            },
            result=&mut start=>result,
        }
    };
    let mut pending = match pending {
        Ok(pending) => pending,
        Err(error) => {
            let message = error.to_string();
            return PromptTaskCompletion {
                cancellation_barrier: frontend_detached
                    .then_some(crate::CancellationBarrier::UnknownTurn),
                binding: None,
                terminal: (!frontend_detached).then(|| {
                    if matches!(error, crate::PromptExecutionError::Busy) {
                        busy_failure(&inputs.request_id, &message)
                    } else {
                        failure(&inputs.request_id, &message)
                    }
                }),
            };
        }
    };
    loop {
        // A frontend detach leaves the in-flight native read intact. Once it
        // yields a refusal, the actor owns the write outside this select.
        let step = {
            let next = pending.next_event(&mut catalog);
            tokio::pin!(next);
            loop {
                tokio::select! {
                    biased;
                    _=inputs.retired.cancelled()=>break NextStep::Retired,
                    command=inputs.commands.recv(),if commands_open=>match command {
                        Some(PromptCommand::Cancel)=>break NextStep::Cancel,
                        None=>{
                            commands_open=false;
                            frontend_detached=true;
                            inputs.commands.close();
                        },
                    },
                    event=&mut next=>break NextStep::Event(event),
                }
            }
        };
        let event = match step {
            NextStep::Retired => {
                return PromptTaskCompletion {
                    cancellation_barrier: None,
                    binding: None,
                    terminal: Some(failure(
                        &inputs.request_id,
                        "Native backend connection lost",
                    )),
                };
            }
            NextStep::Cancel => {
                let terminal = pending.cancel().await;
                let cancellation_barrier = pending
                    .blocks_next_prompt()
                    .then(|| pending.cancellation_barrier());
                return PromptTaskCompletion {
                    cancellation_barrier,
                    binding: pending.into_session().ok(),
                    terminal,
                };
            }
            NextStep::Event(event) => event,
        };
        match event {
            Ok(Some(PromptEvent::Update(frame))) => {
                if !frontend_detached && inputs.output.send(frame).await.is_err() {
                    frontend_detached = true;
                    commands_open = false;
                    inputs.commands.close();
                }
            }
            Ok(Some(PromptEvent::Terminal(frame))) => {
                return PromptTaskCompletion {
                    cancellation_barrier: pending
                        .blocks_next_prompt()
                        .then(|| pending.cancellation_barrier()),
                    binding: pending.into_session().ok(),
                    terminal: (!frontend_detached).then_some(frame),
                };
            }
            Ok(Some(PromptEvent::InteractionRefused {
                id,
                response,
                refusal,
                interrupt_turn,
            })) => {
                if let Err(error) = pending.write_callback_refusal(id, response).await {
                    return PromptTaskCompletion {
                        cancellation_barrier: Some(pending.cancellation_barrier()),
                        binding: None,
                        terminal: (!frontend_detached)
                            .then(|| failure(&inputs.request_id, &error.to_string())),
                    };
                }
                if !interrupt_turn {
                    continue;
                }
                let mut terminal = pending.cancel_after_refusal().await;
                if let Some(metadata) = terminal
                    .as_mut()
                    .and_then(|frame| frame.pointer_mut("/result/_meta"))
                    .and_then(Value::as_object_mut)
                {
                    metadata.insert(
                        "codexRouter".into(),
                        json!({"reasonCode":refusal.code(),"reason":refusal.reason()}),
                    );
                }
                let cancellation_barrier = pending
                    .blocks_next_prompt()
                    .then(|| pending.cancellation_barrier());
                return PromptTaskCompletion {
                    cancellation_barrier,
                    binding: pending.into_session().ok(),
                    terminal: (!frontend_detached).then_some(terminal).flatten(),
                };
            }
            Ok(Some(PromptEvent::NativeCallback(_))) => {
                let _cancel = pending.cancel().await;
                return PromptTaskCompletion {
                    cancellation_barrier: frontend_detached.then(|| pending.cancellation_barrier()),
                    binding: None,
                    terminal: (!frontend_detached).then(|| {
                        failure(
                            &inputs.request_id,
                            "Unsupported native interaction or invalid update",
                        )
                    }),
                };
            }
            Err(error) => {
                let message = error.to_string();
                let _cancel = pending.cancel().await;
                return PromptTaskCompletion {
                    cancellation_barrier: frontend_detached.then(|| pending.cancellation_barrier()),
                    binding: None,
                    terminal: (!frontend_detached).then(|| failure(&inputs.request_id, &message)),
                };
            }
            Ok(Some(PromptEvent::NativeNotification(_)) | None) => {}
        }
    }
}

/// What ended one wait on the prompt's next native step.
enum NextStep<E> {
    Retired,
    Cancel,
    Event(E),
}
fn cancelled_response(id: &Value, state: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"cancelled","_meta":{"codex-router/nativeInterruption":{"state":state},"codex-router/finalReply":{"kind":"available","text":null}}}})
}
fn failure(id: &Value, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"Native prompt rejected","data":{"detail":message}}})
}
fn busy_failure(id: &Value, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":message,"data":{"kind":"busy"}}})
}
