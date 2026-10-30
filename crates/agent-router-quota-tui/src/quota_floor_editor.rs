use agent_collaboration_tui::{DialogAction, UiHeading, UiHeadingLevel};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;

use codex_router_core::ids::AccountId;
use iocraft::prelude::*;

use crate::quota_status_view_model::QuotaStatusViewModelLoader;
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;

pub(crate) const MAX_WEEKLY_FLOOR_PERCENT: u16 = 15;
const WEEKLY_FLOOR_EDITOR_HEIGHT: usize = 10;

pub type WeeklyQuotaFloorSaver =
    Arc<dyn Fn(AccountId, u16) -> WeeklyQuotaFloorSaveFuture + Send + Sync>;
pub type WeeklyQuotaFloorSaveFuture =
    Pin<Box<dyn Future<Output = Result<(), WeeklyQuotaFloorSaveError>> + Send>>;
pub(crate) type QuotaStatusReloadLock = Arc<tokio::sync::Mutex<()>>;

pub(crate) enum WeeklyFloorEditorCommand {
    Save { account_id: AccountId, percent: u16 },
    Reload,
}

#[derive(Clone)]
pub(crate) struct WeeklyFloorEditorCommandPort {
    sender: tokio::sync::mpsc::UnboundedSender<WeeklyFloorEditorCommand>,
    receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<WeeklyFloorEditorCommand>>>>,
}

impl WeeklyFloorEditorCommandPort {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        Self {
            sender,
            receiver: Arc::new(Mutex::new(Some(receiver))),
        }
    }

    pub(crate) fn send(&self, command: WeeklyFloorEditorCommand) -> bool {
        self.sender.send(command).is_ok()
    }

    pub(crate) fn take_receiver(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<WeeklyFloorEditorCommand>> {
        self.receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeeklyQuotaFloorSaveError {
    DatabaseBusy,
    SchemaUpgradeRequired,
    AccountNotFound,
    StateOperationFailed,
}

impl WeeklyQuotaFloorSaveError {
    pub(crate) const fn display_message(self) -> &'static str {
        match self {
            Self::DatabaseBusy => "database busy; retry save",
            Self::SchemaUpgradeRequired => "serve must upgrade the router database",
            Self::AccountNotFound => "account is no longer available",
            Self::StateOperationFailed => "weekly floor update failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WeeklyFloorStep {
    Decrease,
    Increase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WeeklyFloorEditorPhase {
    Editing,
    Saving,
    SaveFailed(WeeklyQuotaFloorSaveError),
    SavedRefreshFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WeeklyFloorEditorState {
    pub(crate) account_id: AccountId,
    pub(crate) account_label: String,
    pub(crate) current_percent: u16,
    pub(crate) draft_percent: u16,
    pub(crate) phase: WeeklyFloorEditorPhase,
}

impl WeeklyFloorEditorState {
    pub(crate) fn new(account_id: AccountId, account_label: String, current_percent: u16) -> Self {
        Self {
            account_id,
            account_label,
            current_percent,
            draft_percent: current_percent.min(MAX_WEEKLY_FLOOR_PERCENT),
            phase: WeeklyFloorEditorPhase::Editing,
        }
    }

    pub(crate) const fn can_adjust(&self) -> bool {
        matches!(
            self.phase,
            WeeklyFloorEditorPhase::Editing | WeeklyFloorEditorPhase::SaveFailed(_)
        )
    }
}

pub(crate) fn step_weekly_floor_percent(percent: u16, step: WeeklyFloorStep) -> u16 {
    match step {
        WeeklyFloorStep::Decrease => percent.saturating_sub(1),
        WeeklyFloorStep::Increase => percent.saturating_add(1).min(MAX_WEEKLY_FLOOR_PERCENT),
    }
}

pub(crate) fn step_weekly_floor_editor(
    state: &mut Option<WeeklyFloorEditorState>,
    step: WeeklyFloorStep,
) {
    let Some(editor) = state.as_mut() else {
        return;
    };
    if !editor.can_adjust() {
        return;
    }
    editor.draft_percent = step_weekly_floor_percent(editor.draft_percent, step);
    editor.phase = WeeklyFloorEditorPhase::Editing;
}

pub(crate) fn weekly_floor_editor_key_command(
    state: &mut Option<WeeklyFloorEditorState>,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Option<WeeklyFloorEditorCommand> {
    let phase = state.as_ref().map(|editor| editor.phase)?;
    let can_cancel = matches!(
        phase,
        WeeklyFloorEditorPhase::Editing | WeeklyFloorEditorPhase::SaveFailed(_)
    );
    match code {
        KeyCode::Left => step_weekly_floor_editor(state, WeeklyFloorStep::Decrease),
        KeyCode::Right => step_weekly_floor_editor(state, WeeklyFloorStep::Increase),
        KeyCode::Enter => {
            let command = match state.as_ref() {
                Some(editor)
                    if matches!(
                        editor.phase,
                        WeeklyFloorEditorPhase::Editing | WeeklyFloorEditorPhase::SaveFailed(_)
                    ) =>
                {
                    Some(WeeklyFloorEditorCommand::Save {
                        account_id: editor.account_id.clone(),
                        percent: editor.draft_percent,
                    })
                }
                Some(editor) if editor.phase == WeeklyFloorEditorPhase::SavedRefreshFailed => {
                    Some(WeeklyFloorEditorCommand::Reload)
                }
                _ => None,
            };
            if command.is_some()
                && let Some(editor) = state.as_mut()
            {
                editor.phase = WeeklyFloorEditorPhase::Saving;
            }
            return command;
        }
        KeyCode::Esc if can_cancel => *state = None,
        KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) && can_cancel => {
            *state = None;
        }
        _ => {}
    }
    None
}

pub(crate) fn weekly_floor_editor_send_failed(state: &mut Option<WeeklyFloorEditorState>) {
    if let Some(editor) = state.as_mut() {
        editor.phase =
            WeeklyFloorEditorPhase::SaveFailed(WeeklyQuotaFloorSaveError::StateOperationFailed);
    }
}

pub(crate) async fn run_weekly_floor_editor_commands(
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<WeeklyFloorEditorCommand>,
    saver: Option<WeeklyQuotaFloorSaver>,
    loader: Option<QuotaStatusViewModelLoader>,
    reload_lock: QuotaStatusReloadLock,
    mut view_model: State<QuotaStatusViewModel>,
    mut editor_state: State<Option<WeeklyFloorEditorState>>,
) {
    while let Some(command) = receiver.recv().await {
        let mut committed_floor = None;
        let save_result = match command {
            WeeklyFloorEditorCommand::Save {
                account_id,
                percent,
            } => match saver.as_ref() {
                Some(saver) => {
                    let result = saver(account_id.clone(), percent).await;
                    if result.is_ok() {
                        committed_floor = Some((account_id, percent));
                    }
                    result
                }
                None => Err(WeeklyQuotaFloorSaveError::StateOperationFailed),
            },
            WeeklyFloorEditorCommand::Reload => Ok(()),
        };
        if let Err(error) = save_result {
            if let Some(editor) = editor_state.write().as_mut() {
                editor.phase = WeeklyFloorEditorPhase::SaveFailed(error);
            }
            continue;
        }
        if let Some((account_id, percent)) = committed_floor
            && let Some(row) = view_model
                .write()
                .rows
                .iter_mut()
                .find(|row| row.account_id == account_id)
        {
            row.weekly_quota_floor_percent = percent;
        }
        let _reload_guard = reload_lock.lock().await;
        let reloaded = match loader.as_ref() {
            Some(loader) => loader().await,
            None => None,
        };
        if let Some(reloaded) = reloaded {
            view_model.set(reloaded);
            editor_state.set(None);
        } else if let Some(editor) = editor_state.write().as_mut() {
            editor.current_percent = editor.draft_percent;
            editor.phase = WeeklyFloorEditorPhase::SavedRefreshFailed;
        }
    }
}

pub(crate) const fn weekly_floor_editor_content_height() -> usize {
    WEEKLY_FLOOR_EDITOR_HEIGHT
}

pub(crate) fn weekly_floor_editor_footer(editor: &WeeklyFloorEditorState) -> &'static str {
    match editor.phase {
        WeeklyFloorEditorPhase::Editing | WeeklyFloorEditorPhase::SaveFailed(_) => {
            "←/→ adjust  enter save  esc/ctrl-e cancel"
        }
        WeeklyFloorEditorPhase::Saving => "saving weekly floor",
        WeeklyFloorEditorPhase::SavedRefreshFailed => "enter retry refresh  ctrl-c exit",
    }
}

pub(crate) fn render_weekly_floor_editor(
    editor: &WeeklyFloorEditorState,
    width: usize,
    height: usize,
    state: State<Option<WeeklyFloorEditorState>>,
) -> AnyElement<'static> {
    let content = render_floor_fields(editor, width.saturating_sub(4), state, 0, true);
    element! {View(width:u32::try_from(width).unwrap_or(u32::MAX),height:u32::try_from(height).unwrap_or(u32::MAX),flex_direction:FlexDirection::Column,border_style:BorderStyle::Single,border_color:Color::DarkGrey,padding_left:1,padding_right:1,overflow:Overflow::Hidden) {
        UiHeading(content:"Weekly floor".to_owned(),level:UiHeadingLevel::Section)
        #(content)
    }}.into_any()
}

pub(crate) fn render_weekly_floor_editor_content(
    editor: &WeeklyFloorEditorState,
    width: usize,
    state: State<Option<WeeklyFloorEditorState>>,
    section_gap: u16,
) -> AnyElement<'static> {
    render_floor_fields(editor, width, state, section_gap, false)
}

fn render_floor_fields(
    editor: &WeeklyFloorEditorState,
    width: usize,
    mut state: State<Option<WeeklyFloorEditorState>>,
    section_gap: u16,
    include_account: bool,
) -> AnyElement<'static> {
    let status = match editor.phase {
        WeeklyFloorEditorPhase::Editing => None,
        WeeklyFloorEditorPhase::Saving => Some(("saving…", Color::Grey)),
        WeeklyFloorEditorPhase::SaveFailed(error) => Some((error.display_message(), Color::Red)),
        WeeklyFloorEditorPhase::SavedRefreshFailed => Some((
            "saved; refresh failed",
            agent_collaboration_tui::UiTextRole::Warning.color(),
        )),
    };
    let current = if editor.current_percent == 0 {
        "disabled (0%)".to_owned()
    } else {
        format!("{}%", editor.current_percent)
    };
    let decrease_target = editor.account_id.clone();
    let increase_target = editor.account_id.clone();
    let context_margin = if status.is_some() { 0 } else { section_gap };
    element! {
        View(width:u32::try_from(width).unwrap_or(u32::MAX),max_width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,row_gap:section_gap,margin_top:context_margin) {
            #(status.map(|(message,color)|element!(Text(content:message,color))))
            agent_collaboration_tui::FormFields(row_gap:section_gap) {
            #(include_account.then(||element!(agent_collaboration_tui::FormField(label:"Account".to_owned()) {Text(content:editor.account_label.clone(),color:Color::White)})))
            agent_collaboration_tui::FormField(label:"Current".to_owned()) {Text(content:current,color:Color::White)}
            agent_collaboration_tui::FormField(label:"Floor".to_owned(),has_focus:true) {
                View(column_gap:2,align_items:AlignItems::Center) {
                    DialogAction(label:"◀",handler:move |_| {
                        let mut next=state.read().clone();
                        if next.as_ref().is_some_and(|editor|editor.account_id==decrease_target) {step_weekly_floor_editor(&mut next,WeeklyFloorStep::Decrease);state.set(next);}
                    })
                    Text(content:format!("{}%",editor.draft_percent),color:Color::Yellow,weight:Weight::Bold)
                    DialogAction(label:"▶",handler:move |_| {
                        let mut next=state.read().clone();
                        if next.as_ref().is_some_and(|editor|editor.account_id==increase_target) {step_weekly_floor_editor(&mut next,WeeklyFloorStep::Increase);state.set(next);}
                    })
                }
            }
            }
            Text(content:"0% disables protection · maximum 15%",color:Color::Grey)
        }
    }.into_any()
}
