//! Picker frame view.
use super::{
    MIN_PICKER_WIDTH, SIDECAR_PICKER_WIDTH, SessionConversationPreview, SessionsPickerModel,
    SessionsPickerOutcome, fit_line, picker_body_budget, render_details, render_session_list,
    render_start_new_details, root_label, runtime_view_label, session_visible_row_budget,
    sort_label,
};
use codex_router_tui::{
    DialogAction, DialogFrame, DialogHost, DialogMargins, DialogPresentation, SurfaceHeader,
};
use iocraft::prelude::*;
use unicode_width::UnicodeWidthStr;

pub(super) struct PickerViewportBudget {
    pub(super) height: usize,
    pub(super) minimum_render_height: usize,
}

pub(super) fn render_picker_view(
    model: &SessionsPickerModel,
    model_state: State<SessionsPickerModel>,
    selected_outcome: State<Option<SessionsPickerOutcome>>,
    selected_conversation: Option<&SessionConversationPreview>,
    viewport: PickerViewportBudget,
    session_clipboard_writer: super::super::session_id_clipboard::SessionClipboardWriter,
    apply_filters: Handler<()>,
) -> Element<'static, View> {
    let PickerViewportBudget {
        height,
        minimum_render_height,
    } = viewport;
    let content_width = model.width.saturating_sub(4).max(MIN_PICKER_WIDTH);
    let header_title = format!(
        "Sessions · {}/{}",
        model.visible_record_len(),
        model.request.records.len()
    );
    let header_actions_width = "New".width() + "Details".width() + "Help".width() + 3 * 2;
    let separate_header_actions = header_title.width() + header_actions_width > content_width;
    let control_width = content_width.saturating_sub(2) / 2;
    let mut filter_controls = if control_width < "All providers".width() + 2 {
        vec![
            element!(View(width:100pct,flex_shrink:0.0_f32) {
                #(super::super::picker_machine_view::render_machine_control(model, model_state, content_width))
            }).into_any(),
            element!(View(width:100pct,flex_shrink:0.0_f32) {
                #(super::super::provider_filter_view::render_provider_controls(model, model_state, content_width))
            }).into_any(),
        ]
    } else {
        vec![element! {
        View(width: 100pct, column_gap: 2, flex_shrink: 0.0_f32) {
            View(width: control_width as u32, min_width: 0, overflow: Overflow::Hidden) {
                #(super::super::picker_machine_view::render_machine_control(model, model_state, control_width))
            }
            View(flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
                #(super::super::provider_filter_view::render_provider_controls(model, model_state, control_width))
            }
        }
    }.into_any()]
    };
    filter_controls.extend(render_filter_controls(model, content_width));
    let show_source_status = model.machine_controls.filter
        != super::super::picker_machine_controls::PickerMachineFilter::Default
        || model.source_progress.iter().any(|progress| {
            matches!(
                progress.read_state,
                super::super::source_reload_progress::SourceReadState::Rejected { .. }
            )
        });
    if show_source_status && !model.source_progress.is_empty() {
        let status = model
            .source_progress
            .iter()
            .map(|progress| progress.display_line())
            .collect::<Vec<_>>()
            .join(" | ");
        filter_controls.push(element! { Text(content: fit_line(&status, content_width), color: Color::Grey, wrap: TextWrap::NoWrap) }.into_any());
    }
    if let Some(notice) = model
        .machine_controls
        .all_notice(&model.request.router_registry, &model.source_progress)
    {
        filter_controls.push(element! { Text(content: fit_line(&notice, content_width), color: codex_router_tui::UiTextRole::Warning.color(), wrap: TextWrap::NoWrap) }.into_any());
    }
    if let Some(notice) = model.runtime_coverage.notice() {
        filter_controls.push(element! {
            Text(content: fit_line(notice, content_width), color: codex_router_tui::UiTextRole::Warning.color(), wrap: TextWrap::NoWrap)
        }.into_any());
    }
    let incomplete_native = model
        .native_live_coverage
        .iter()
        .filter(|part| part.state.is_incomplete())
        .count();
    if incomplete_native > 0 {
        filter_controls.push(element!(Text(content:fit_line(&format!("Native Claude inventory incomplete ({incomplete_native} reads)"),content_width),color:codex_router_tui::UiTextRole::Warning.color(),wrap:TextWrap::NoWrap)).into_any());
    }
    if let Some(error) = super::super::source_action_view::source_error(model) {
        filter_controls.push(error);
    }
    let control_height = filter_controls.len() + usize::from(separate_header_actions);
    let body_budget = picker_body_budget(height, control_height, minimum_render_height);
    let inactive = model.show_help
        || model.provider_filter_draft.is_some()
        || model.new_session.is_some()
        || model.details_dialog.is_some()
        || model.source_action.pending.is_some();
    let mut details_model = model_state;
    let mut new_model = model_state;
    let header_actions = vec![element!(DialogAction(label: "New", keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,has_focus:model.focused_identity().is_none(), handler: move |_|new_model.write().open_new_form())).into_any(),element!(DialogAction(label: "Details", handler: move |_| details_model.write().open_details_dialog())).into_any()];
    let mut children = if separate_header_actions {
        vec![
            element!(SurfaceHeader(title:header_title,inactive,
                help_action:Some(super::super::picker_help_view::help_action(model_state))))
            .into_any(),
            element!(View(width:100pct,column_gap:2,flex_shrink:0.0_f32) { #(header_actions) })
                .into_any(),
        ]
    } else {
        vec![element!(SurfaceHeader(title:header_title,inactive,
            help_action:Some(super::super::picker_help_view::help_action(model_state)),children:header_actions)).into_any()]
    };
    children.extend(filter_controls);

    let focused_record = model.focused_record();
    if model.width >= SIDECAR_PICKER_WIDTH {
        let list_width = (content_width.saturating_sub(2) / 2).max(42);
        let detail_width = content_width.saturating_sub(list_width + 2).max(28);
        let visible_row_budget = session_visible_row_budget(model, body_budget);
        children.push(
            element! {
                View(width: 100pct, height: body_budget as u32) {
                    #(render_session_list(model, model_state, selected_outcome, list_width, visible_row_budget, body_budget))
                    View(width: 2) { Text(content: "") }
                    #(focused_record
                        .map(|record| render_details(record, detail_width, selected_conversation, Size::Length(body_budget as u32)))
                        .unwrap_or_else(|| render_start_new_details(model, detail_width, body_budget)))
                }
            }
            .into_any(),
        );
    } else {
        let list_height = body_budget;
        let visible_row_budget = session_visible_row_budget(model, list_height);
        children.push(render_session_list(
            model,
            model_state,
            selected_outcome,
            content_width,
            visible_row_budget,
            list_height,
        ));
    }

    let picker_height = height.max(minimum_render_height);
    let background = element! {
        View(
            width: model.width as u32,
            height: picker_height as u32,
            border_style: BorderStyle::Round,
            border_color: Color::Cyan,
            overflow: Overflow::Hidden,
            padding_left: 1,
            padding_right: 1,
            padding_top: 0,
            padding_bottom: 0,
            flex_direction: FlexDirection::Column,
            row_gap: 0,
        ) {
            #(children)
        }
    }
    .into_any();
    let viewport_width = u16::try_from(model.width).unwrap_or(u16::MAX);
    let viewport_height = u16::try_from(picker_height).unwrap_or(u16::MAX);
    let dialog = if model.show_help {
        Some(super::super::picker_help_view::render_help(
            model,
            model_state,
            picker_height,
        ))
    } else if let Some(pending) = &model.source_action.pending {
        Some(super::super::source_action_view::checking_dialog(
            pending,
            model_state,
            picker_height,
        ))
    } else if model.new_session.is_some() {
        Some(super::super::new_session_form::render_new_form(
            model,
            model_state,
            selected_outcome,
            picker_height,
        ))
    } else if model.provider_filter_draft.is_some() {
        let short_filters = viewport_height < 16 && viewport_width >= 48;
        let presentation = if short_filters {
            DialogPresentation::FullScreen
        } else {
            match DialogPresentation::default() {
                DialogPresentation::Sized { width, height } => DialogPresentation::Sized {
                    width,
                    height: if viewport_width <= 48 {
                        codex_router_tui::DialogAxisSize::cells(viewport_height.saturating_sub(2))
                    } else {
                        height
                    },
                },
                DialogPresentation::FullScreen => DialogPresentation::FullScreen,
            }
        };
        let margins = if short_filters {
            DialogMargins::default()
        } else {
            DialogMargins {
                top: 1,
                right: 2,
                bottom: 1,
                left: 2,
            }
        };
        let ((_, frame_width), _) = presentation.resolve(
            viewport_width.saturating_sub(margins.left + margins.right),
            viewport_height.saturating_sub(margins.top + margins.bottom),
        );
        Some(element! {
            DialogFrame(width: viewport_width, height: viewport_height, keyboard_owner: codex_router_tui::DialogKeyboardOwner::Caller,
                title: "Session filters".to_owned(), help_action: Some(super::super::picker_help_view::help_action(model_state)),
                scroll_keys:if model.provider_choice.is_open() {codex_router_tui::DialogScrollKeys::Disabled}else {codex_router_tui::DialogScrollKeys::Pages},
                presentation, margins,
                actions: super::super::provider_filter_view::render_provider_actions(model, model_state, apply_filters)) {
                #(super::super::provider_filter_view::render_provider_chooser(model, model_state,usize::from(frame_width.saturating_sub(4)), short_filters))
            }
        }.into_any())
    } else if model.details_dialog.is_some() {
        Some(super::details_dialog_view::render_details_dialog(
            model,
            model_state,
            selected_outcome,
            selected_conversation,
            viewport_width,
            viewport_height,
            session_clipboard_writer,
        ))
    } else {
        None
    };
    element! {
        View(width: model.width as u32, height: picker_height as u32) {
            DialogHost(width: viewport_width, height: viewport_height, background: vec![background], dialog)
        }
    }
}

pub(super) fn render_filter_controls(
    model: &SessionsPickerModel,
    width: usize,
) -> Vec<AnyElement<'static>> {
    let mut controls = Vec::new();
    if !model.search.is_empty() {
        controls.push(control_line(vec![fit_line(
            &format!("Search: [{}]", model.search),
            width,
        )]));
    }
    let mut settings = Vec::new();
    if model.root != super::SessionsPickerRoot::Any {
        settings.push(format!("[{}]", root_label(model.root)));
    }
    if model.runtime_view != super::super::picker_model::SessionsPickerRuntimeView::All {
        settings.push(format!(
            "View: [{}]",
            runtime_view_label(model.runtime_view)
        ));
    }
    if model.sort != super::SessionsSort::Updated {
        settings.push(format!("Sort: [{}]", sort_label(model.sort)));
    }
    if settings.is_empty() {
        return controls;
    }
    if one_line_filter_controls_fit_for_parts(width, settings.iter().map(String::as_str)) {
        controls.push(control_line(settings));
    } else {
        controls.extend(
            settings
                .into_iter()
                .map(|setting| control_line(vec![setting])),
        );
    }
    controls
}

pub(super) fn one_line_filter_controls_fit_for_parts<'a>(
    width: usize,
    parts: impl IntoIterator<Item = &'a str>,
) -> bool {
    let parts = parts.into_iter().collect::<Vec<_>>();
    parts
        .iter()
        .map(|part| UnicodeWidthStr::width(*part))
        .sum::<usize>()
        + parts.len().saturating_sub(1) * 3
        <= width
}

pub(super) fn control_line(parts: Vec<String>) -> AnyElement<'static> {
    let children = parts
        .into_iter()
        .map(|part| {
            element! {
                Text(content: part, color: Color::Grey, wrap: TextWrap::NoWrap)
            }
            .into_any()
        })
        .collect::<Vec<_>>();
    element! { View(column_gap: 3, flex_shrink: 0.0_f32) { #(children) } }.into_any()
}
