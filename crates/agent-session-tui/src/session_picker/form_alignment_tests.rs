use super::super::*;
use super::picker_detail_view;
use iocraft::prelude::*;

#[derive(Props)]
struct DialogFixtureProps {
    dialog: usize,
}

#[component]
fn DialogFixture(props: &DialogFixtureProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut request = test_support::picker_request();
    request.new_session_args_display = "--model fixture-model".to_owned();
    let mut model = picker_model::SessionsPickerModel::new(request.clone(), 120);
    model.open_new_form();
    model.machine_controls.open_browse(&request.router_registry);
    let state = hooks.use_state(|| model.clone());
    let outcome = hooks.use_state(|| None);
    let dialog = match props.dialog {
        0 => new_session_form::render_new_form(&model, state, outcome, 36),
        1 => picker_fork_view::render_fork_confirmation(
            &picker_fork_confirmation::ForkConfirmation::capture(&request.records[0], &request),
            120,
            36,
            element!(View).into_any(),
            false,
            state,
            outcome,
        )
        .into_any(),
        2 => picker_machine_view::render_machine_choices(
            &model,
            36,
            element!(View).into_any(),
            Handler::default(),
            Handler::default(),
            state,
        )
        .into_any(),
        _ => picker_detail_view::render_details(&request.records[0], 100, None, Size::Auto),
    };
    element!(View(width:120,height:36) {#(dialog)})
}

fn assert_field_origins(frame: &str, pairs: &[(&str, &str)]) {
    let widest = pairs
        .iter()
        .map(|(label, _)| unicode_width::UnicodeWidthStr::width(*label))
        .max()
        .unwrap();
    for (label, value) in pairs {
        let line = frame
            .lines()
            .find(|line| line.contains(label) && line.contains(value))
            .unwrap_or_else(|| panic!("{label} / {value}: {frame}"));
        let label_origin = unicode_width::UnicodeWidthStr::width(line.split_once(label).unwrap().0);
        let value_origin = unicode_width::UnicodeWidthStr::width(line.split_once(value).unwrap().0);
        assert_eq!(
            value_origin,
            label_origin + widest + 4,
            "shared label width, gap, and reserved two-cell gutter: {frame}"
        );
    }
}

#[test]
fn luna_422_new_session_fields_share_value_origin() {
    let frame = element!(DialogFixture(dialog:0usize))
        .render(Some(120))
        .to_string();
    assert_field_origins(
        &frame,
        &[
            ("Client", "Codex ▾"),
            ("Machine", "This machine ▾"),
            ("Directory", "/repo/project-a"),
            ("Arguments", "--model fixture-model"),
        ],
    );
}

#[test]
fn luna_422_fork_fields_share_value_origin() {
    let frame = element!(DialogFixture(dialog:1usize))
        .render(Some(120))
        .to_string();
    assert_field_origins(
        &frame,
        &[
            ("Source", "This machine"),
            ("Directory", "/repo/project-a"),
            ("Destination", "This machine ▾"),
        ],
    );
}

#[test]
fn luna_422_machine_field_reserves_shared_value_gutter() {
    let frame = element!(DialogFixture(dialog:2usize))
        .render(Some(120))
        .to_string();
    assert_field_origins(&frame, &[("Machine", "Search:")]);
}

#[test]
fn luna_422_session_details_share_value_origin() {
    let frame = element!(DialogFixture(dialog:3usize))
        .render(Some(120))
        .to_string();
    assert_field_origins(
        &frame,
        &[
            ("Session ID:", "thread-a"),
            ("Working directory:", "/repo/project-a"),
        ],
    );
}
