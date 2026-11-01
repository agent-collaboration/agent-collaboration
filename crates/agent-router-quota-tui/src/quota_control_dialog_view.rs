//! Dialog content is a projection; actions and input belong to the declared scope.
use super::{
    quota_combo_controls::{QuotaChoiceField, choice_target},
    quota_control_context::{QuotaControlContext, QuotaControlResult, QuotaDialogKind, action_id},
};
use agent_collaboration_tui::{
    UiText, UiTextRole,
    actions::ActionId,
    controls::{
        combo::ComboBox,
        fields::{Button, FormField, FormFields, SegmentedSwitch, StatusKind, StatusValue},
        list::ScrollHandle,
    },
    dialog::{
        DialogFrame, DialogSection, DialogSections, PinnedContext,
        help::{HelpControl, HelpDialog},
    },
    surfaces::{focus_model::TargetId, surface_handle::SurfaceContext},
};
use codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase;
use iocraft::{ComponentDrawer, Hook, prelude::*};

struct OptionsSwitchWidth(State<u16>);

impl Hook for OptionsSwitchWidth {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        let available_width = drawer.size().width;
        if self.0.get() != available_width {
            self.0.set(available_width);
        }
    }
}

#[derive(Props)]
struct QuotaOptionsSwitchProps {
    actions: Vec<ActionId>,
    #[iocraft(required)]
    available_width: State<u16>,
}

#[component]
fn QuotaOptionsSwitch(
    props: &QuotaOptionsSwitchProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    hooks
        .use_hook(|| OptionsSwitchWidth(props.available_width))
        .0 = props.available_width;
    // Measure the pinned row after DialogFrame padding.
    // The same live value controls segmented_targets in the dialog model.
    element!(View(width:100pct,min_width:0,flex_shrink:0.0_f32) {
        SegmentedSwitch(actions:props.actions.clone(),available_width:props.available_width.get())
    })
}

#[derive(Props)]
pub(crate) struct QuotaControlDialogProps {
    #[iocraft(required)]
    pub(crate) context: QuotaControlContext,
    #[iocraft(required)]
    pub(crate) kind: QuotaDialogKind,
    #[iocraft(required)]
    pub(crate) scroll: ScrollHandle,
    #[iocraft(required)]
    pub(crate) help: HelpControl,
}

#[component]
pub(crate) fn QuotaControlDialog(
    props: &QuotaControlDialogProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let surface = *hooks.use_context::<SurfaceContext>();
    if props.help.is_active(surface.surface)
        && let Some(scroll) = props.help.scroll(surface.surface)
    {
        return element!(DialogFrame(title:"Help".to_owned(),scroll,scroll_target:props.help.scroll_target(),scroll_focus:Some(props.help.body_target())) {HelpDialog(control:props.help.clone())}).into_any();
    }
    let result = dialog_content(&props.context, &props.kind, surface).and_then(|(title, content)| {
        let (controls, status) = if props.kind == QuotaDialogKind::Options {
            let actions = vec![action_id("resets-tab")?, action_id("credits-tab")?];
            (
                vec![element!(QuotaOptionsSwitch(actions,available_width:props.context.options_switch_available_width)).into_any()],
                options_status(&props.context),
            )
        } else {
            (Vec::new(), None)
        };
        Ok((title, content, controls, status))
    });
    let (title, content, pinned_controls, pinned_status) = match result {
        Ok(content) => content,
        Err(message) => (
            "Quota".to_owned(),
            element!(UiText(content:message,role:UiTextRole::Error,wrap:TextWrap::Wrap)).into_any(),
            Vec::new(),
            None,
        ),
    };
    let help = action_id("help")
        .ok()
        .map(|action| element!(Button(id:TargetId::for_action(&action),action)).into_any());
    let identity = dialog_identity(&props.context, &props.kind);
    let scroll_focus = matches!(props.kind, QuotaDialogKind::Options)
        .then(super::quota_domain_controls::domain_body_target)
        .transpose()
        .ok()
        .flatten();
    match super::quota_dialog_controls::dialog_scroll_target() {
        Ok(scroll_target)=>element!(DialogFrame(title,scroll:props.scroll.clone(),scroll_target,scroll_focus,help_action:help,context:identity,pinned_controls,pinned_status) {#(content)}).into_any(),
        Err(error)=>element!(UiText(content:error,role:UiTextRole::Error)).into_any(),
    }
}

fn dialog_identity(context: &QuotaControlContext, kind: &QuotaDialogKind) -> Option<PinnedContext> {
    let model = context.view_model.read();
    let options = context.account_options.read();
    let target = context.reset_target.read();
    let floor = context.weekly_floor_editor.read();
    let details = match kind {
        QuotaDialogKind::Account(target) => Some(target),
        _ => None,
    };
    super::quota_dialog_context::current_dialog_identity(
        options.as_ref(),
        target.as_ref(),
        floor.as_ref(),
        details,
        &model.rows,
    )
    .map(super::quota_dialog_context::QuotaDialogIdentity::pinned_context)
}

fn dialog_content(
    context: &QuotaControlContext,
    kind: &QuotaDialogKind,
    surface: SurfaceContext,
) -> QuotaControlResult<(String, AnyElement<'static>)> {
    match kind {
        QuotaDialogKind::Account(target) if context.remove_confirmation.get()=>Ok(("Remove account".into(),element!(DialogSections {
            DialogSection(heading:"Confirm removal".to_owned()) {
                UiText(content:target.label.as_str().to_owned(),role:UiTextRole::Prominent,wrap:TextWrap::Wrap)
                UiText(content:format!("{} · {}",target.provider,target.account_id.as_str()),role:UiTextRole::Secondary,wrap:TextWrap::Wrap)
                View(column_gap:1,flex_shrink:0.0_f32) {
                    UiText(content:"Warning:",role:UiTextRole::Warning)
                    UiText(content:"Remove this disabled account and its credentials? This cannot be undone.",role:UiTextRole::Body,wrap:TextWrap::Wrap)
                }
            }
        }).into_any())),
        QuotaDialogKind::Account(target)=>Ok(("Account details".into(),context.target_row(target).map_or_else(
            ||element!(StatusValue(status:StatusKind::Warning,detail:Some("Account is no longer listed".to_owned()))).into_any(),
            |row|super::quota_account_details_view::render_account_details(&row.details,context.width.get().saturating_sub(12).min(84),Some(target)),
        ))),
        QuotaDialogKind::Pool=>Ok(("Pool details".into(),pool_details(context))),
        QuotaDialogKind::Filters=>Ok(("Account filters".into(),element!(DialogSections {DialogSection(heading:String::new()) {FormFields(stacked:context.width.get()<60) {
            FormField(label:"Account provider".to_owned(),has_focus:surface.is_focused(&choice_target(QuotaChoiceField::Provider)?)) {ComboBox(id:choice_target(QuotaChoiceField::Provider)?,value:context.provider_choice)}
            FormField(label:"Disabled accounts".to_owned(),has_focus:surface.is_focused(&choice_target(QuotaChoiceField::Disabled)?)) {ComboBox(id:choice_target(QuotaChoiceField::Disabled)?,value:context.disabled_choice)}
            FormField(label:"Group by".to_owned(),has_focus:surface.is_focused(&choice_target(QuotaChoiceField::Group)?)) {ComboBox(id:choice_target(QuotaChoiceField::Group)?,value:context.group_choice)}
        }}}).into_any())),
        QuotaDialogKind::Floor=>Ok(("Weekly floor".into(),floor_content(context)?)),
        QuotaDialogKind::Options=>options_content(context,surface),
    }
}

fn field(label: &str, value: String, role: UiTextRole) -> Element<'static, FormField> {
    element!(FormField(label:label.to_owned()) {UiText(content:value,role,wrap:TextWrap::Wrap)})
}

fn pool_details(context: &QuotaControlContext) -> AnyElement<'static> {
    let model = context.view_model.read();
    let header = &model.header;
    let pool = header.pool;
    let freshness = header.freshness;
    let facts = [
        ("Accounts", pool.total()),
        ("Usable", pool.usable),
        ("Reserve", pool.reserve),
        ("Blocked", pool.blocked),
        ("Pool unknown", pool.unknown),
        ("Excluded", pool.excluded),
        ("Fresh", freshness.fresh),
        ("Stale", freshness.stale),
        ("Data unknown", freshness.unknown),
    ];
    element!(DialogSections {DialogSection(heading:String::new()) {FormFields {
        #(vec![field("Pool",header.route_band.clone(),UiTextRole::Body)])
        #(facts.into_iter().map(|(label,count)|field(label,count.to_string(),UiTextRole::Body)))
        #(vec![field("Clients",header.observed_clients.map_or_else(||"unknown".to_owned(),|count|count.to_string()),UiTextRole::Body)])
    }}
    #(header.credential_notice.clone().map(|content|element!(StatusValue(status:StatusKind::Error,detail:Some(content)))))
    #(model.selection_projection_degraded.then(||element!(StatusValue(status:StatusKind::Error,detail:Some("Selection projection degraded".to_owned())))))
    }).into_any()
}

fn floor_content(context: &QuotaControlContext) -> QuotaControlResult<AnyElement<'static>> {
    use super::quota_floor_editor::WeeklyFloorEditorPhase;
    let editor = context.weekly_floor_editor.read();
    let editor = editor
        .as_ref()
        .ok_or_else(|| "Weekly floor editor closed".to_owned())?;
    let decrease = action_id("floor-decrease")?;
    let increase = action_id("floor-increase")?;
    let current = if editor.current_percent == 0 {
        "disabled (0%)".to_owned()
    } else {
        format!("{}%", editor.current_percent)
    };
    let status = match editor.phase {
        WeeklyFloorEditorPhase::Editing => None,
        WeeklyFloorEditorPhase::Saving => {
            Some((StatusKind::Available, "Saving preference…".to_owned()))
        }
        WeeklyFloorEditorPhase::SaveFailed(error) => {
            Some((StatusKind::Error, error.display_message().to_owned()))
        }
        WeeklyFloorEditorPhase::SavedRefreshFailed => {
            Some((StatusKind::Warning, "saved; refresh failed".to_owned()))
        }
    };
    let label_width = FormFields::label_width_for(["Current", "Floor"]);
    Ok(element!(DialogSections {
        DialogSection(heading:String::new()) {FormFields(stacked:context.width.get()<40,minimum_label_width:Some(label_width)) {#(vec![field("Current",current,UiTextRole::Body)])}}
        DialogSection(heading:String::new()) {FormFields(stacked:context.width.get()<40,minimum_label_width:Some(label_width)) {FormField(label:"Floor".to_owned()) {View(column_gap:0) {Button(id:TargetId::for_action(&decrease),action:decrease,show_key:false) UiText(content:format!("{}%",editor.draft_percent),role:UiTextRole::Selected) Button(id:TargetId::for_action(&increase),action:increase,show_key:false)}}}}
        DialogSection(heading:String::new()) {UiText(content:"0% disables protection · maximum 15%",role:UiTextRole::Secondary,wrap:TextWrap::Wrap)}
        #(status.map(|(status,detail)|element!(DialogSection(heading:String::new()) {StatusValue(status,detail:Some(detail))})))
    }).into_any())
}

fn options_content(
    context: &QuotaControlContext,
    surface: SurfaceContext,
) -> QuotaControlResult<(String, AnyElement<'static>)> {
    let options = context.account_options.read();
    let options = options
        .as_ref()
        .ok_or_else(|| "Account options closed".to_owned())?;
    let phase = context.phase();
    let snapshot = context.reset_snapshot.read();
    let target = context.reset_target.read();
    let title = if options.editor.is_some() {
        "Credit usage"
    } else if phase == WorkflowPhase::Confirming {
        "Confirm reset"
    } else if phase == WorkflowPhase::Revalidating {
        "Checking reset"
    } else if phase == WorkflowPhase::Committing {
        "Reset in progress"
    } else if phase == WorkflowPhase::Result {
        "Reset result"
    } else if options.tab == super::quota_account_options::AccountOptionsTab::Resets {
        "Options · Resets"
    } else {
        "Options · Credits"
    };
    let body = if options.tab == super::quota_account_options::AccountOptionsTab::Credits {
        credit_content(context, options, surface)?
    } else if let (Some(snapshot), Some(target)) = (snapshot.as_ref(), target.as_ref())
        && phase != WorkflowPhase::Browse
    {
        let choices = if phase == WorkflowPhase::Confirming {
            let no = action_id("reset-no")?;
            let yes = action_id("reset-yes")?;
            Some(element!(View(column_gap:1) {Button(id:TargetId::for_action(&no),action:no) Button(id:TargetId::for_action(&yes),action:yes)}).into_any())
        } else {
            None
        };
        super::quota_reset_detail_rendering::render_reset_panel_content(
            snapshot,
            target,
            context.width.get().saturating_sub(8).min(84),
            context.inventory_page_start.get(),
            super::quota_reset_presentation_model::reset_inventory_page_size(
                context.height.get().saturating_sub(10),
            ),
            context.spinner_tick.get(),
            choices,
        )
    } else {
        let explanation = if !options.target.enabled {
            Some("Resets are unavailable while this account is disabled.")
        } else if options.target.credential_generation.is_none() {
            Some("Resets are unavailable without active credentials.")
        } else if target.is_some() {
            Some("Checking reset eligibility…")
        } else {
            None
        };
        explanation.map_or_else(||element!(FormFields(stacked:context.width.get()<40) {#(vec![field("Eligibility","Not checked".into(),UiTextRole::Body)])}).into_any(),|content|element!(UiText(content,role:UiTextRole::Secondary,wrap:TextWrap::Wrap)).into_any())
    };
    Ok((title.to_owned(),element!(View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,row_gap:if context.height.get()<16{0}else{1}) {#(options.pending_tab.is_some().then(||element!(UiText(content:"Tab change pending",role:UiTextRole::Secondary)))) #(body)}).into_any()))
}

fn options_status(context: &QuotaControlContext) -> Option<AnyElement<'static>> {
    use super::quota_account_options::{
        CreditPolicyEditorPhase, account_options_message_role, account_options_message_text,
    };
    let options = context.account_options.read();
    let options = options.as_ref()?;
    if options.tab != super::quota_account_options::AccountOptionsTab::Credits {
        return None;
    }
    let status = options
        .editor
        .as_ref()
        .and_then(|editor| match editor.phase {
            CreditPolicyEditorPhase::Editing => None,
            CreditPolicyEditorPhase::Saving => {
                Some((StatusKind::Available, "Saving preference…".to_owned()))
            }
            CreditPolicyEditorPhase::SaveFailed(error) => {
                Some((StatusKind::Error, error.message().to_owned()))
            }
            CreditPolicyEditorPhase::SavedReloadFailed => Some((
                StatusKind::Warning,
                "Saved, but account data could not be reloaded.".to_owned(),
            )),
        });
    if status.is_none() && options.message.is_none() {
        return None;
    }
    Some(element!(View(width:100pct,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
        #(status.map(|(status,detail)|element!(StatusValue(status,detail:Some(detail)))))
        #(options.message.map(|message|element!(UiText(content:account_options_message_text(message),role:account_options_message_role(message),wrap:TextWrap::Wrap))))
    }).into_any())
}

fn credit_content(
    context: &QuotaControlContext,
    options: &super::quota_account_options::AccountOptionsState,
    surface: SurfaceContext,
) -> QuotaControlResult<AnyElement<'static>> {
    use super::quota_account_options::{
        credit_entitlement_label, credit_freshness_label, provider_control_note,
    };
    let usage = &options.target.credit_usage;
    let editor = options.editor.as_ref();
    let policy = if let Some(editor) = editor {
        editor.draft_policy
    } else {
        usage.policy
    };
    let policy_control = if editor.is_some() {
        element!(ComboBox(id:choice_target(QuotaChoiceField::Credit)?,value:context.credit_choice))
            .into_any()
    } else {
        element!(UiText(content:if policy==codex_router_core::credit_usage::CreditUsagePolicy::Allow {"Allow"}else{"Disallow"},role:UiTextRole::Body)).into_any()
    };
    Ok(element!(DialogSections {DialogSection(heading:if editor.is_some(){String::new()}else{"Credit balance".to_owned()}) {FormFields(stacked:context.width.get()<40) {
        #(vec![field(if editor.is_some(){"Balance"}else{"Status"},credit_entitlement_label(usage),UiTextRole::Body)])
        #((editor.is_none()||usage.freshness!=codex_router_quota_service::quota::CreditUsageFreshness::Fresh).then(||field("Freshness",credit_freshness_label(usage.freshness).to_owned(),UiTextRole::Secondary)))
        #(editor.is_none().then(||field("Observation",if usage.age_label=="unknown" {"not checked".to_owned()}else{format!("checked {} ago",usage.age_label)},UiTextRole::Secondary)))
    }} DialogSection(heading:if editor.is_some(){String::new()}else{"Credit usage".to_owned()}) {FormFields(stacked:context.width.get()<40) {FormField(label:"Use credits".to_owned(),has_focus:surface.is_focused(&choice_target(QuotaChoiceField::Credit)?)) {#(policy_control)}}}
    #(provider_control_note(usage).map(|detail|element!(StatusValue(status:StatusKind::Warning,detail:Some(detail)))))
    #((options.target.weekly_quota_floor_percent>0).then(||element!(StatusValue(status:StatusKind::Warning,detail:Some(format!("Weekly floor {}% blocks credit routing.",options.target.weekly_quota_floor_percent))))))
    }).into_any())
}
