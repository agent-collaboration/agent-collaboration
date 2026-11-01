//! Project actual owner facts; fixture setup and native dispatch have separate writers.
use super::event_fence::{FixtureEventCompletions, FixtureEventFenceResult, FixtureEventKind};
use crate::quota::quota_refresh_adapter::observation::{RefreshObservation, ServiceReturn};
use agent_router_quota_tui::{
    QuotaAccountAction, QuotaAccountCommand, QuotaAccountCommandResult, QuotaRefreshResult,
    test_support::{
        QuotaCapturedContext, QuotaFixtureControlAck, QuotaFixtureOperation, QuotaFixtureReadback,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{self, Write},
    sync::{Arc, Mutex},
};

struct FixtureJournalWriter {
    ordinal: u64,
    output: Box<dyn Write + Send>,
}

#[derive(Clone)]
pub(super) struct FixtureEvents {
    writer: Arc<Mutex<FixtureJournalWriter>>,
    completions: FixtureEventCompletions,
}
impl FixtureEvents {
    pub(super) fn new(file: File) -> Self {
        Self::with_writer(file)
    }
    pub(super) fn with_writer(writer: impl Write + Send + 'static) -> Self {
        Self {
            writer: Arc::new(Mutex::new(FixtureJournalWriter {
                ordinal: 0,
                output: Box::new(writer),
            })),
            completions: FixtureEventCompletions::new(),
        }
    }
    pub(super) fn write(&self, event: Value) -> io::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("fixture event writer poisoned"))?;
        writer.ordinal = writer
            .ordinal
            .checked_add(1)
            .ok_or_else(|| io::Error::other("fixture event IDs exhausted"))?;
        let kind = serde_json::from_value::<FixtureEventKind>(event["kind"].clone()).ok();
        let record = json!({"sequence":writer.ordinal,"event":event});
        serde_json::to_writer(&mut writer.output, &record).map_err(io::Error::other)?;
        writeln!(&mut writer.output)?;
        writer.output.flush()?;
        if let Some(kind) = kind {
            self.completions.flushed(writer.ordinal, kind);
        }
        Ok(())
    }
    pub(super) async fn await_event(
        &self,
        after_ordinal: u64,
        kind: FixtureEventKind,
    ) -> FixtureEventFenceResult {
        self.completions.await_event(after_ordinal, kind).await
    }
    pub(super) fn finish(&self) {
        self.completions.finish();
    }
}

#[derive(Serialize)]
pub(super) enum FixtureControlReceiptKind {
    #[serde(rename = "fixture-control-ack")]
    FixtureControl,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FixtureControlReceipt {
    kind: FixtureControlReceiptKind,
    request_id: u64,
    operation_id: u64,
    operation: &'static str,
    readback: Value,
}
impl FixtureControlReceipt {
    pub(super) fn from_ack(request_id: u64, ack: &QuotaFixtureControlAck) -> Self {
        Self {
            kind: FixtureControlReceiptKind::FixtureControl,
            request_id,
            operation_id: ack.operation_id,
            operation: match ack.operation {
                QuotaFixtureOperation::SelectBrowseAccount => "select-browse-account",
                QuotaFixtureOperation::ReloadViewModel => "reload-view-model",
            },
            readback: readback(&ack.readback),
        }
    }
}

pub(super) fn readback(value: &QuotaFixtureReadback) -> Value {
    let captured=value.captured.as_ref().map(|captured|match captured {
        QuotaCapturedContext::Details(target)=>json!({"kind":"details","accountId":target.account_id.as_str(),"label":target.label.as_str(),"provider":target.provider.as_str(),"position":target.position}),
        QuotaCapturedContext::Options {account_id,account_label}=>json!({"kind":"options","accountId":account_id.as_str(),"label":account_label}),
        QuotaCapturedContext::Floor {account_id,account_label}=>json!({"kind":"floor","accountId":account_id.as_str(),"label":account_label}),
        QuotaCapturedContext::Other=>json!({"kind":"other"}),
    });
    let top = value.surfaces.top();
    let actions=value.actions.iter().map(|action|json!({
        "id":action.id.as_str(),"title":action.title,"keys":action.keys.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "available":action.availability.is_available(),"availability":format!("{:?}",action.availability),"checked":action.checked,
    })).collect::<Vec<_>>();
    let published=value.published.as_ref().map(|frame|json!({"frame":frame.frame,
        "surfaces":frame.surfaces.iter().map(|region|json!({"surface":format!("{:?}",region.surface),"cells":region.cells})).collect::<Vec<_>>(),
        "targets":frame.targets.iter().map(|region|json!({"surface":format!("{:?}",region.surface),"target":region.target.as_str(),"kind":format!("{:?}",region.kind),"cells":region.cells})).collect::<Vec<_>>()
    }));
    json!({"mount":value.mount,"browseAccountId":value.browse_account.as_ref().map(|id|id.as_str()),"listSelection":value.list_selection,
        "captured":captured,"scope":top.actions.scope().as_str(),"focus":top.focus.current().map(|id|id.as_str()),"actions":actions,"published":published,
        "rows":value.view_model.rows.iter().map(|row|json!({"accountId":row.account_id.as_str(),"label":row.account,"provider":row.provider.as_str(),"enabled":row.enabled,
            "loginState":format!("{:?}",row.login_state),"weeklyWindow":row.weekly_window,"detailsWeeklyWindow":row.details.weekly_window,"resetCredits":row.reset_credits,"status":format!("{:?}",row.status)})).collect::<Vec<_>>()})
}

pub(super) fn action_name(action: QuotaAccountAction) -> &'static str {
    match action {
        QuotaAccountAction::CopyId => "copy-id",
        QuotaAccountAction::Disable => "disable",
        QuotaAccountAction::Enable => "enable",
        QuotaAccountAction::Remove => "remove",
    }
}
pub(super) fn command(request: &QuotaAccountCommand) -> Value {
    json!({"action":action_name(request.action),"accountId":request.target.account_id.as_str(),"provider":request.target.provider.as_str(),"label":request.target.label.as_str(),"position":request.target.position})
}
pub(super) fn command_result(result: &QuotaAccountCommandResult) -> Value {
    match result {
        QuotaAccountCommandResult::Succeeded => json!({"kind":"succeeded"}),
        QuotaAccountCommandResult::Refused { reason } => {
            json!({"kind":"refused","reason":format!("{reason:?}")})
        }
        QuotaAccountCommandResult::Failed { message } => json!({"kind":"failed","message":message}),
    }
}
pub(super) fn refresh_result(result: &QuotaRefreshResult) -> Value {
    match result {
        QuotaRefreshResult::Refreshed => json!({"kind":"refreshed"}),
        QuotaRefreshResult::Partial { failed } => {
            json!({"kind":"partial","failed":failed.iter().map(|f|json!({"accountId":f.account_id.as_str(),"message":f.message})).collect::<Vec<_>>()})
        }
        QuotaRefreshResult::Failed { failed, overall } => {
            json!({"kind":"failed","failed":failed.iter().map(|f|json!({"accountId":f.account_id.as_str(),"message":f.message})).collect::<Vec<_>>(),"overall":overall})
        }
    }
}
pub(super) fn refresh_observation(observation: &RefreshObservation) -> Value {
    match observation {
        RefreshObservation::ServiceReturned(source) => {
            json!({"kind":"service-returned","source":service_return(source)})
        }
        RefreshObservation::MappingConsumed(source) => {
            json!({"kind":"mapping-consumed","source":service_return(source)})
        }
        RefreshObservation::SetupFailed { message } => {
            json!({"kind":"setup-failed","message":message})
        }
        RefreshObservation::MappingReturned(result) => {
            json!({"kind":"mapping-returned","result":refresh_result(result)})
        }
    }
}
fn service_return(source: &ServiceReturn) -> Value {
    use codex_router_quota_service::quota_refresh_outcome::AccountRefreshStatus;
    let kind = match source {
        ServiceReturn::Ok(_) => "ok",
        ServiceReturn::AllAccountsFailed(_) => "all-failed",
        ServiceReturn::Error { .. } => "service-error",
    };
    let outcomes=source.account_outcomes().iter().map(|entry|json!({"accountId":entry.account_id.as_str(),"outcome":match &entry.outcome {
        AccountRefreshStatus::Refreshed=>json!({"kind":"refreshed"}),AccountRefreshStatus::Failed {reason}=>json!({"kind":"failed","reason":reason}),AccountRefreshStatus::Skipped {reason}=>json!({"kind":"skipped","reason":reason}),
    }})).collect::<Vec<_>>();
    json!({"kind":kind,"accountOutcomes":outcomes,"message":match source {ServiceReturn::Error {message}=>Some(message),_=>None}})
}
