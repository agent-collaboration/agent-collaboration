//! Content-sized machine controls and choices in the existing iocraft visual language.
use super::interactive_row::InteractiveSessionChoiceRow;
use super::picker_machine_controls::PickerMachineStage;
use super::picker_model::SessionsPickerModel;
use iocraft::prelude::*;

pub(super) fn render_machine_control(
    model: &SessionsPickerModel,
    mut model_state: State<SessionsPickerModel>,
    width: usize,
) -> AnyElement<'static> {
    let machine = model.machine_controls.label(model.request.machine_mode);
    let label = super::picker_component::fit_line(
        &if width >= 32 {
            format!("Machine: {machine}")
        } else {
            machine.to_owned()
        },
        width.saturating_sub(2),
    );
    let focused = model.machine_controls.control_focused;
    element! {
        InteractiveSessionChoiceRow(
            focus_handler: move |_| { model_state.write().machine_controls.control_focused = true; },
            activation_handler: move |_| {
                let mut model = model_state.write();
                if model.provider_filter_draft.is_some() { return; }
                let registry = model.request.router_registry.clone();
                model.machine_controls.open_browse(&registry);
            },
            activates_on_click: true,
        ) {
            View(width: 100pct, column_gap: 1) {
                Text(content: label, color: if focused { Color::Yellow } else { Color::Grey }, weight: Weight::Bold, wrap: TextWrap::NoWrap)
                Text(content: "▾", color: Color::DarkGrey, wrap: TextWrap::NoWrap)
            }
        }
    }.into_any()
}

pub(super) fn render_machine_choices(
    model: &SessionsPickerModel,
    height: usize,
    help_action: AnyElement<'static>,
    selection_handler: Handler<super::picker_machine_controls::MachineChoiceKey>,
) -> Element<'static, View> {
    let width = u16::try_from(model.width).unwrap_or(u16::MAX);
    let height = u16::try_from(height).unwrap_or(u16::MAX);
    let (title, body) = match &model.machine_controls.stage {
        PickerMachineStage::Loading { source, .. } => {
            let name = match source.as_ref() {
                super::PickerSourceContext::ConfiguredHosted(profile) => profile.name.as_str(),
                super::PickerSourceContext::DefaultHosted => "This machine",
                super::PickerSourceContext::LocalCodex => "Local Codex",
            };
            (format!("Loading machine: {name}"),element!(Text(content:"Previous view retained until the selected source is verified",color:Color::Grey)).into_any())
        }
        PickerMachineStage::Choosing { notice, .. } => {
            let options = model
                .machine_controls
                .choice_options(&model.request.router_registry, model.request.machine_mode);
            ("Choose machine".to_owned(),element! {
                View(flex_direction:FlexDirection::Column) {
                    codex_router_tui::SearchSelect<super::picker_machine_controls::MachineChoiceKey>(state:model.machine_controls.search.clone(),options,on_select:selection_handler)
                    #(notice.as_ref().map(|notice|element!(Text(content:notice.message(),color:Color::Yellow))))
                }
            }.into_any())
        }
        PickerMachineStage::Browsing => return element!(View),
    };
    element! {
        View(width:u32::from(width),height:u32::from(height)) {
            codex_router_tui::DialogFrame(width,height,title,keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,
                margins:codex_router_tui::DialogMargins {top:1,right:2,bottom:1,left:2},help_action:Some(help_action),
                actions:vec![element!(Text(content:"Esc Cancel",color:Color::Grey)).into_any()]) {#(body)}
        }
    }
}
