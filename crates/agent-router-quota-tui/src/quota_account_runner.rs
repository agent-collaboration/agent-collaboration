//! Accepted account mutations outlive the mounted UI and drain before CLI return.
use super::{QuotaAccountCommand, QuotaAccountCommandResult, QuotaAccountCommands};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaAccountCompletion {
    pub command: QuotaAccountCommand,
    pub result: QuotaAccountCommandResult,
}

impl QuotaAccountCompletion {
    pub fn message(&self) -> String {
        let label = self.command.target.label.as_str();
        match &self.result {
            QuotaAccountCommandResult::Succeeded => format!(
                "{}: {label}",
                match self.command.action {
                    super::QuotaAccountAction::Disable => "Account disabled",
                    super::QuotaAccountAction::Enable => "Account enabled",
                    super::QuotaAccountAction::Remove => "Account removed",
                    super::QuotaAccountAction::CopyId => "Account ID copied",
                }
            ),
            QuotaAccountCommandResult::Refused { reason } => {
                format!("{label}: {}", reason.message())
            }
            QuotaAccountCommandResult::Failed { message } => format!("{label}: {message}"),
        }
    }
}

#[derive(Clone)]
pub struct QuotaAccountCommandQueue {
    sender: mpsc::UnboundedSender<QuotaAccountCommand>,
    completions: watch::Receiver<Option<QuotaAccountCompletion>>,
    shutdown: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl QuotaAccountCommandQueue {
    pub fn submit(&self, command: QuotaAccountCommand) -> bool {
        let accepting = self
            .shutdown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        accepting.is_some() && self.sender.send(command).is_ok()
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<QuotaAccountCompletion>> {
        self.completions.clone()
    }

    /// The owner calls this after teardown, then awaits the retained runner.
    pub fn close(&self) {
        if let Some(shutdown) = self
            .shutdown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = shutdown.send(());
        }
    }
}

pub type QuotaAccountRunner = Pin<Box<dyn Future<Output = Vec<QuotaAccountCompletion>> + Send>>;

pub fn quota_account_command_runner(
    commands: QuotaAccountCommands,
) -> (QuotaAccountCommandQueue, QuotaAccountRunner) {
    let (sender, mut receiver) = mpsc::unbounded_channel::<QuotaAccountCommand>();
    let (completed, completions) = watch::channel(None);
    let (shutdown, mut stopping) = oneshot::channel();
    let queue = QuotaAccountCommandQueue {
        sender,
        completions,
        shutdown: Arc::new(Mutex::new(Some(shutdown))),
    };
    let runner = Box::pin(async move {
        let mut receipts = Vec::new();
        loop {
            let request = tokio::select! {
                biased;
                _ = &mut stopping => {
                    receiver.close();
                    break;
                }
                request = receiver.recv() => request,
            };
            let Some(request) = request else {
                return receipts;
            };
            complete_account_command(&commands, request, &completed, &mut receipts).await;
        }
        while let Some(request) = receiver.recv().await {
            complete_account_command(&commands, request, &completed, &mut receipts).await;
        }
        receipts
    });
    (queue, runner)
}

async fn complete_account_command(
    commands: &QuotaAccountCommands,
    command: QuotaAccountCommand,
    completed: &watch::Sender<Option<QuotaAccountCompletion>>,
    receipts: &mut Vec<QuotaAccountCompletion>,
) {
    let result = commands(command.clone()).await;
    let receipt = QuotaAccountCompletion { command, result };
    completed.send_replace(Some(receipt.clone()));
    receipts.push(receipt);
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_router_core::{ids::AccountId, provider::Provider};

    #[tokio::test]
    async fn closing_the_ui_queue_drains_accepted_work_before_the_runner_returns() {
        let (began, mut begun) = mpsc::unbounded_channel();
        let (release, released) = oneshot::channel();
        let release_slot = Arc::new(Mutex::new(Some(released)));
        let commands: QuotaAccountCommands = Arc::new(move |_| {
            let began = began.clone();
            let released = release_slot.lock().unwrap().take().unwrap();
            Box::pin(async move {
                began.send(()).unwrap();
                released.await.unwrap();
                QuotaAccountCommandResult::Succeeded
            })
        });
        let (queue, runner) = quota_account_command_runner(commands);
        let mut observed = queue.subscribe();
        let task = tokio::spawn(runner);
        let command = QuotaAccountCommand {
            target: super::super::AccountActionTarget {
                account_id: AccountId::new("captured").unwrap(),
                label: super::super::QuotaAccountLabel::new("captured label").unwrap(),
                provider: Provider::Openai,
                position: 0,
            },
            action: super::super::QuotaAccountAction::Remove,
        };
        assert!(queue.submit(command.clone()));
        begun.recv().await.unwrap();
        queue.close();
        assert!(
            !queue.submit(command.clone()),
            "teardown closes admission before waiting for the held command"
        );
        assert!(
            !task.is_finished(),
            "teardown cannot cancel accepted removal"
        );
        release.send(()).unwrap();
        let receipts = task.await.unwrap();
        observed.changed().await.unwrap();
        assert_eq!(
            receipts,
            vec![QuotaAccountCompletion {
                command,
                result: QuotaAccountCommandResult::Succeeded
            }]
        );
        assert_eq!(observed.borrow().as_ref(), receipts.first());
        assert!(
            !queue.submit(receipts[0].command.clone()),
            "closed runner accepts no new mutation"
        );
    }
}
