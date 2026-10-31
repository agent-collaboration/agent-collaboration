use super::super::{quota_account_options::*, quota_floor_editor::*, quota_provider_filters::*};
use super::*;

#[derive(Props)]
struct QuotaFormFixtureProps {
    dialog: usize,
}

#[component]
fn QuotaFormFixture(
    props: &QuotaFormFixtureProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let model = quota_view_model();
    let editor =
        WeeklyFloorEditorState::new(model.rows[0].account_id.clone(), "ssdev".to_owned(), 7);
    let editor_state = hooks.use_state(|| Some(editor.clone()));
    let filters = hooks.use_state(QuotaFilterState::default);
    let choice = hooks.use_state(agent_collaboration_tui::SearchSelectState::default);
    let view_model = hooks.use_state(|| model.clone());
    let focused_account_id = hooks.use_state(|| None);
    let options = hooks.use_state(|| None);
    match props.dialog {
        0 => super::super::quota_floor_editor::render_weekly_floor_editor(
            &editor,
            100,
            20,
            editor_state,
        ),
        1 => super::super::quota_provider_filter_rendering::render_quota_filter_chooser(
            QuotaFilterDraft {
                filters: QuotaAccountFilters::default(),
                focus: QuotaFilterFocus::Provider,
            },
            100,
            QuotaFilterController {
                state: filters,
                choice,
                view_model,
                focused_account_id,
            },
        ),
        _ => {
            let mut state = AccountOptionsState::new(&model.rows[0], 1);
            state.tab = AccountOptionsTab::Credits;
            super::super::quota_account_options::render_account_options_content(
                super::super::quota_account_options::AccountOptionsPanelProps {
                    options: &state,
                    reset_snapshot: None,
                    reset_target: None,
                    width: 100,
                    height: 36,
                    inventory_page_start: 0,
                    spinner_tick: 0,
                },
                Some(options),
            )
        }
    }
}

fn assert_origins(frame: &str, pairs: &[(&str, &str)]) {
    let widest = pairs
        .iter()
        .map(|(label, _)| unicode_width::UnicodeWidthStr::width(*label))
        .max()
        .unwrap();
    for (label, value) in pairs {
        let line = frame
            .lines()
            .find(|line| line.contains(label) && line.contains(value))
            .unwrap_or_else(|| panic!("{label}/{value}: {frame}"));
        let label_origin = unicode_width::UnicodeWidthStr::width(line.split_once(label).unwrap().0);
        let value_origin = unicode_width::UnicodeWidthStr::width(line.split_once(value).unwrap().0);
        assert_eq!(value_origin, label_origin + widest + 4, "{frame}");
    }
}

#[test]
fn luna_422_quota_details_share_value_origin() {
    let details = selected_account_details("ssdev", "safest quota");
    let frame = super::super::quota_browse_rendering::render_selected_details(&details, 100, true)
        .render(None)
        .to_string();
    assert_origins(
        &frame,
        &[
            ("Status", "available"),
            ("Reason", "safest quota"),
            ("weekly", "████ 83%"),
            ("5h", "█████ 99%"),
            ("weekly", "□□□□□□□│□□□□□□□"),
            ("sample", "sample fresh 14s"),
            ("rate", "0.10%/h"),
            ("conn", "0.05%/h/conn"),
            ("5h", "□□□□□□□│■■■■■■■"),
            ("clients", "1 client"),
            ("guards", "5h 0% / weekly 8%"),
            ("reset", "2 available"),
            ("note", "safest quota"),
        ],
    );
}

#[test]
fn luna_422_floor_fields_share_value_origin() {
    let frame = element!(QuotaFormFixture(dialog:0usize))
        .render(Some(100))
        .to_string();
    assert_origins(
        &frame,
        &[("Account", "ssdev"), ("Current", "7%"), ("Floor", "◀")],
    );
}

#[test]
fn luna_422_filter_fields_share_value_origin_and_focus_gutter() {
    let frame = element!(QuotaFormFixture(dialog:1usize))
        .render(Some(100))
        .to_string();
    assert_origins(
        &frame,
        &[
            ("Account provider", "All ▾"),
            ("Disabled accounts", "Shown ▾"),
        ],
    );
    let selected = frame
        .lines()
        .find(|line| line.contains("Account provider"))
        .unwrap();
    assert!(selected.contains("❯"), "{frame}");
}

#[test]
fn luna_422_credit_fields_share_value_origin() {
    let frame = element!(QuotaFormFixture(dialog:2usize))
        .render(Some(100))
        .to_string();
    assert_origins(
        &frame,
        &[
            ("Status", "Unknown"),
            ("Freshness", "Unknown"),
            ("Observation", "not checked"),
            ("Use credits", "Disallow"),
        ],
    );
}
