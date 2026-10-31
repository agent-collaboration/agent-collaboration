//! The picker paints shared controls from the declared surface model.
use super::picker_choice_scope::fields;
use super::{
    PickerDialogKind, PickerMachineFilter, PickerSurfaceContext, SessionsPickerModel, SurfaceId,
    SurfaceResult, target,
};
use agent_collaboration_tui::{
    controls::{ComboBox, FormField, FormFields, ListRow, ScrollList},
    hit_regions::{HitRegion, HitRegionKind},
    surface_handle::SurfaceContext,
    theme::TextRole,
};
use iocraft::prelude::*;

struct SessionColumnLayout {
    stacked: bool,
    status_width: u16,
    updated_width: u16,
    created_width: u16,
}
impl SessionColumnLayout {
    fn for_viewport(width: u16) -> Self {
        Self {
            stacked: width < 40,
            status_width: if width < 54 { 2 } else { 6 },
            updated_width: if width < 54 { 3 } else { 6 },
            created_width: if width < 54 { 3 } else { 8 },
        }
    }
}

#[derive(Props)]
pub struct PickerSurfaceViewProps {
    #[iocraft(required)]
    pub context: PickerSurfaceContext,
}
#[component]
pub fn PickerSurfaceView(
    props: &PickerSurfaceViewProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let surface = *hooks.use_context::<SurfaceContext>();
    match render_screen(&props.context, surface) {
        Ok(view) => view,
        Err(error) => element!(Text(content:error.to_string())).into_any(),
    }
}

fn render_screen(
    context: &PickerSurfaceContext,
    surface: SurfaceContext,
) -> SurfaceResult<AnyElement<'static>> {
    let model = context.model.read();
    let theme = surface.theme();
    let (width, height) = surface.stack.viewport();
    let control = super::picker_screen_scope::list_control(context, surface.stack)?;
    let toolbar = super::picker_toolbar::render_toolbar(context, surface)?;
    let notices = inventory_notices(&model);
    let notices = notices
        .into_iter()
        .map(|content| {
            element!(Text(content,color:theme.foreground(TextRole::Warning),wrap:TextWrap::Wrap))
                .into_any()
        })
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for index in 0..model.visible_record_len() {
        let Some(record) = model.visible_record_at(index) else {
            continue;
        };
        let key = super::picker_screen_scope::row_key(&record.identity)?;
        let record = record.clone();
        rows.push(element!(View(key:key.as_str().to_owned(),width:100pct,padding_left:u16::from(width>=54),flex_shrink:0.0_f32) {
            ListRow(control:control.clone(),row:key.clone()) {PickerSessionRow(record)}
        }).into_any());
    }
    let empty = if model.search.is_empty() {
        "No existing sessions match these filters"
    } else {
        "No matching sessions"
    };
    if rows.is_empty() {
        rows.push(element!(Text(content:empty,color:theme.foreground(TextRole::Muted))).into_any());
    }
    let list_id = control.id().clone();
    let columns = SessionColumnLayout::for_viewport(width);
    let source_error = model.source_action.error.map(|error| error.to_string());
    let wide = width >= 140;
    let content_width = width.saturating_sub(4);
    let list_width = content_width.saturating_sub(2) / 2;
    let detail_width = content_width.saturating_sub(list_width + 2);
    let sidecar = if wide {
        model.focused_record().map(|record| {
            let preview = context
                .conversation_cache
                .read()
                .select_record(record)
                .preview;
            super::super::render_details(
                record,
                usize::from(detail_width),
                Some(&preview),
                Size::Percent(100.0),
            )
        })
    } else {
        None
    };
    let list = element!(HitRegion(target:list_id.clone(),kind:HitRegionKind::Target) {
        View(width:100pct,flex_grow:1.0_f32,min_height:0,flex_direction:FlexDirection::Column,border_style:BorderStyle::Single,border_color:theme.foreground(TextRole::Muted),padding_left:1,padding_right:1) {
            View(width:100pct,column_gap:1,padding_right:1) {
                View(width:1+u16::from(width>=54),flex_shrink:0.0_f32)
                View(flex_grow:1.0_f32,min_width:0) {Text(content:"Session",color:theme.foreground(TextRole::Accent),wrap:TextWrap::NoWrap)}
                #((!columns.stacked).then(||element!(View(width:columns.status_width,flex_shrink:0.0_f32) {Text(content:if width<54 {"St"}else{"Status"},color:theme.foreground(TextRole::Accent),wrap:TextWrap::NoWrap)})))
                #((!columns.stacked).then(||element!(View(width:columns.updated_width,flex_shrink:0.0_f32,justify_content:JustifyContent::FlexEnd) {Text(content:"Upd",color:theme.foreground(TextRole::Accent),wrap:TextWrap::NoWrap)})))
                #((!columns.stacked).then(||element!(View(width:columns.created_width,flex_shrink:0.0_f32,justify_content:JustifyContent::FlexEnd) {Text(content:if width<54 {"Age"}else{"Created"},color:theme.foreground(TextRole::Accent),wrap:TextWrap::NoWrap)})))
            }
            ScrollList(id:control.scroll_target(),keyboard_target:Some(list_id),scroll:control.scroll(),grow:true,children:rows)
        }
    });
    Ok(element!(View(width,height,flex_direction:FlexDirection::Column,overflow:Overflow::Hidden,border_style:BorderStyle::Round,border_color:theme.foreground(TextRole::Accent),padding_left:1,padding_right:1) {
        #(Some(toolbar))
        #(notices)
        #(source_error.map(|content|element!(Text(content,color:theme.foreground(TextRole::Warning)))))
        #(context.error.read().clone().map(|content|element!(Text(content,color:theme.foreground(TextRole::Error)))))
        View(width:100pct,flex_grow:1.0_f32,min_height:0) {
            View(width:if wide {Size::Length(u32::from(list_width))}else {Size::Percent(100.0)},height:100pct,min_height:0) {#(Some(list))}
            #(wide.then(||element!(View(width:2))))
            #(sidecar)
        }

    }).into_any())
}

fn inventory_notices(model: &SessionsPickerModel) -> Vec<String> {
    let mut lines = Vec::new();
    let has_source_status = model.machine_controls.filter != PickerMachineFilter::Default
        || model.source_progress.iter().any(|progress| {
            matches!(
                progress.read_state,
                crate::session_picker::source_reload_progress::SourceReadState::Rejected { .. }
            )
        });
    if has_source_status && !model.source_progress.is_empty() {
        lines.push(
            model
                .source_progress
                .iter()
                .map(|progress| progress.display_line())
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }
    if let Some(notice) = model
        .machine_controls
        .all_notice(&model.request.router_registry, &model.source_progress)
    {
        lines.push(notice);
    }
    if let Some(notice) = model.runtime_coverage.notice() {
        lines.push(notice.to_owned());
    }
    let incomplete = model
        .native_live_coverage
        .iter()
        .filter(|part| part.state.is_incomplete())
        .count();
    if incomplete > 0 {
        lines.push(format!(
            "Native Claude inventory incomplete ({incomplete} reads)"
        ));
    }
    for part in &model.native_live_coverage {
        let warnings = part.state.read_warning_lines();
        if warnings.is_empty() && part.state.conflicts().is_empty() {
            continue;
        }
        lines.push(format!(
            "Native Claude source: {} / {}",
            String::from(part.endpoint.service_id.clone()),
            String::from(part.endpoint.endpoint_id.clone())
        ));
        lines.extend(warnings);
        for conflict in part.state.conflicts() {
            lines.push(format!(
                "Session {}",
                String::from(conflict.target.session_id.clone())
            ));
            lines.push(format!("Conflict: {}", conflict.reason.description()));
        }
    }
    lines
}

#[derive(Props)]
struct PickerSessionRowProps {
    #[iocraft(required)]
    record: agent_session_inventory::sessions::SessionPickerRecord,
}
#[component]
fn PickerSessionRow(props: &PickerSessionRowProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let surface = *hooks.use_context::<SurfaceContext>();
    let theme = surface.theme();
    let record = &props.record;
    let (viewport_width, _) = surface.stack.viewport();
    let columns = SessionColumnLayout::for_viewport(viewport_width);
    let status = element!(View(width:columns.status_width,flex_shrink:0.0_f32,overflow:Overflow::Hidden) {Text(content:record.runtime_status.icon(),color:theme.foreground(TextRole::Text),wrap:TextWrap::NoWrap)}).into_any();
    let updated = element!(View(width:columns.updated_width,flex_shrink:0.0_f32,overflow:Overflow::Hidden,justify_content:JustifyContent::FlexEnd) {Text(content:super::super::compact_age(&record.recency),color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)}).into_any();
    let created = element!(View(width:columns.created_width,flex_shrink:0.0_f32,overflow:Overflow::Hidden,justify_content:JustifyContent::FlexEnd) {Text(content:super::super::compact_age(&record.created),color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)}).into_any();
    let status_row = vec![status, updated, created];
    let (title_row_status, stacked_status) = if columns.stacked {
        (Vec::new(), Some(status_row))
    } else {
        (status_row, None)
    };
    element!(View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,margin_bottom:1) {
        View(width:100pct,column_gap:1,overflow:Overflow::Hidden) {
            View(flex_grow:1.0_f32,min_width:0,overflow:Overflow::Hidden) {Text(content:record.title.clone(),color:theme.foreground(TextRole::Text),weight:Weight::Bold,wrap:TextWrap::NoWrap)}
            #(title_row_status)
        }
        #(stacked_status.map(|children|element!(View(width:100pct,column_gap:1,children))))
        View(width:100pct,column_gap:1,overflow:Overflow::Hidden) {
            Text(content:record.client_provider_family().label(),color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)
            Text(content:"·",color:theme.foreground(TextRole::Muted))
            Text(content:record.machine_label(),color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)
            Text(content:"·",color:theme.foreground(TextRole::Muted))
            Text(content:format!("⎇ {}",record.branch),color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)
            Text(content:"·",color:theme.foreground(TextRole::Muted))
            View(flex_grow:1.0_f32,min_width:0,overflow:Overflow::Hidden) {Text(content:format!("📂 {}",record.cwd.as_deref().unwrap_or("-")),color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)}
        }
    })
}

pub(super) fn dialog_body(
    context: &PickerSurfaceContext,
    kind: PickerDialogKind,
    surface: SurfaceContext,
) -> AnyElement<'static> {
    match render_dialog_body(context, kind, surface) {
        Ok(body) => body,
        Err(error) => element!(Text(content:error.to_string())).into_any(),
    }
}
fn render_dialog_body(
    context: &PickerSurfaceContext,
    kind: PickerDialogKind,
    surface: SurfaceContext,
) -> SurfaceResult<AnyElement<'static>> {
    let model = context.model.read();
    let viewport = surface.stack.viewport();
    let (frame_width, frame_height) = surface
        .stack
        .snapshot()
        .overlays()
        .iter()
        .find(|overlay| SurfaceId::Overlay(overlay.id) == surface.surface)
        .map(|overlay| match overlay.layout.resolve(viewport) {
            agent_collaboration_tui::surface_stack::OverlayPlacement::Centered {
                max_width,
                max_height,
                ..
            } => (max_width, max_height),
            agent_collaboration_tui::surface_stack::OverlayPlacement::Fixed {
                width,
                height,
                ..
            } => (width, height),
        })
        .unwrap_or(viewport);
    let padding = u16::from(frame_height >= 16);
    // Text must measure at the same width that the frame and scrollbar paint.
    let width = usize::from(frame_width.saturating_sub(3 + 2 * padding));
    let theme = surface.theme();
    if kind == PickerDialogKind::Details {
        let Some(dialog) = model.details_dialog.as_ref() else {
            return Ok(element!(View).into_any());
        };
        let preview = context
            .conversation_cache
            .read()
            .select_record(dialog.record())
            .preview;
        return Ok(element!(View(width:100pct,flex_direction:FlexDirection::Column) {
            #(match dialog.clipboard_status {crate::session_picker::details_dialog_state::ClipboardRequestStatus::Idle=>None,crate::session_picker::details_dialog_state::ClipboardRequestStatus::Sent=>Some(element!(Text(content:"Copy requested",color:theme.foreground(TextRole::Muted)))),crate::session_picker::details_dialog_state::ClipboardRequestStatus::Failed=>Some(element!(Text(content:"Could not copy",color:theme.foreground(TextRole::Error))))})
            #(dialog.inspection_notice().map(|notice|element!(Text(content:notice.to_owned(),color:theme.foreground(TextRole::Muted)))))
            #(super::super::render_details(dialog.record(),width,Some(&preview),Size::Auto))
        }).into_any());
    }
    let label_width = FormFields::label_width_for(match kind {
        PickerDialogKind::New => vec!["Client", "Machine", "Directory", "Arguments"],
        PickerDialogKind::Fork => vec!["Source", "Directory", "Destination"],
        PickerDialogKind::Machine => vec!["Machine"],
        PickerDialogKind::Filters => vec![
            "Client provider",
            "Folder",
            "Model provider",
            "Requests",
            "Runtime",
            "Sort",
        ],
        _ => Vec::new(),
    });
    let mut rows = Vec::new();
    for field in fields(context, kind) {
        rows.push(element!(FormField(label:field.label().to_owned(),has_focus:surface.is_focused(&target(field.name())?)) {ComboBox(id:target(field.name())?,value:super::picker_choice_scope::combo_state(context, field)?)}).into());
    }
    if let Some(draft) = model
        .new_session
        .as_ref()
        .filter(|_| kind == PickerDialogKind::New)
    {
        rows.push(element!(FormField(label:"Directory".to_owned(),has_focus:surface.is_focused(&target("new-directory")?)) {HitRegion(target:target("new-directory")?,kind:HitRegionKind::Target) {Text(content:draft.directory.display().to_string(),color:theme.foreground(TextRole::Text))}}).into());
        if !model.request.new_session_args_display.is_empty() {
            rows.push(element!(FormField(label:"Arguments".to_owned()) {Text(content:model.request.new_session_args_display.clone(),color:theme.foreground(TextRole::Muted))}).into());
        }
    }
    if let Some(confirmation) = model
        .fork_confirmation
        .as_ref()
        .filter(|_| kind == PickerDialogKind::Fork)
    {
        rows.insert(0,element!(FormField(label:"Source".to_owned()) {Text(content:confirmation.source_label.clone(),color:theme.foreground(TextRole::Muted))}).into());
        rows.insert(1,element!(FormField(label:"Directory".to_owned()) {Text(content:confirmation.directory_label.clone(),color:theme.foreground(TextRole::Text)) Text(content:format!("Directory origin: {}",confirmation.directory_origin),color:theme.foreground(TextRole::Muted))}).into());
        if let Some(notice) = confirmation.notice.or_else(|| {
            confirmation
                .destinations
                .get(confirmation.focused_destination)
                .and_then(|destination| destination.availability.explanation())
        }) {
            rows.push(
                element!(Text(content:notice,color:theme.foreground(TextRole::Warning)))
                    .into_any()
                    .into(),
            );
        }
    }
    if kind == PickerDialogKind::Machine {
        use crate::session_picker::picker_machine_controls::PickerMachineStage;
        match &model.machine_controls.stage {
            PickerMachineStage::Choosing {
                notice: Some(notice),
                ..
            } => rows.push(
                element!(Text(content:notice.message(),color:theme.foreground(TextRole::Warning)))
                    .into_any()
                    .into(),
            ),
            PickerMachineStage::Loading { source, .. } => {
                let name = match source.as_ref() {
                    crate::session_picker::PickerSourceContext::ConfiguredHosted(profile) => {
                        profile.name.as_str()
                    }
                    _ => "selected machine",
                };
                rows.push(element!(Text(content:format!("Loading machine: {name}…"),color:theme.foreground(TextRole::Muted))).into_any().into());
            }
            _ => {}
        }
    }
    if let Some(pending) = model
        .source_action
        .pending
        .as_ref()
        .filter(|_| kind == PickerDialogKind::Source)
    {
        rows.push(element!(Text(content:format!("{} · Verifying the selected source before handoff",pending.profile.name.as_str()),color:theme.foreground(TextRole::Text))).into_any().into());
    }
    if kind == PickerDialogKind::Client {
        rows.push(crate::session_picker::client_action_view::render_client_body(&model).into());
    }
    Ok(
        element!(FormFields(stacked:width<38,minimum_label_width:Some(label_width),children:rows))
            .into_any(),
    )
}
