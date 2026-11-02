use agent_collaboration_tui::controls::fields::{FormField, FormFields, StatusKind, StatusValue};
use iocraft::prelude::*;
const SESSION_ID: &str = "00000000-0000-4000-8000-000000000014";
#[test]
fn shared_label_budget_yields_to_whole_status_words_and_keeps_fitting_wide_columns() {
    for width in [110u16, 66, 38, 30, 22, 18] {
        let text=element!(View(width,flex_direction:FlexDirection::Column) {FormFields(stacked:width<66) {
                FormField(label:"Long section label 界".to_owned()) {StatusValue(status:StatusKind::Available,detail:Some("Local recording only".to_owned()))}
                FormField(label:"ID".to_owned()) {Text(content:"KNOWN_VALUE")}
            }}).render(None).to_string();
        assert!(
            text.lines().any(|line| line.contains("Available")),
            "status word stays whole at {width}\n{text}"
        );
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized.contains("Long section label 界"),
            "label words remain intact and complete\n{text}"
        );
        let label_row = text
            .lines()
            .position(|line| line.contains("Long section"))
            .unwrap();
        let value_row = text
            .lines()
            .position(|line| line.contains("Available"))
            .unwrap();
        if width >= 66 {
            assert_eq!(label_row, value_row, "wide shared columns stay on one row");
        } else {
            assert!(
                value_row > label_row,
                "label stacks above a value that does not fit beside it\n{text}"
            );
        }
        assert!(
            text.contains("KNOWN_VALUE"),
            "other row remains complete\n{text}"
        );
    }
}
#[test]
fn a_uuid_that_fits_after_the_selector_gutter_stays_whole_and_only_overlong_ids_break() {
    for width in [76u16, 38, 37, 32, 28] {
        let text=element!(View(width) {FormFields(stacked:width<50) {FormField(label:"Session ID".to_owned()) {Text(content:SESSION_ID)}}}).render(None).to_string();
        if width >= 38 {
            assert!(
                text.lines().any(|line| line.contains(SESSION_ID)),
                "36-cell ID fits a 38-cell value row including the selector gutter\n{text}"
            );
        } else {
            assert!(
                text.chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>()
                    .contains(SESSION_ID),
                "only a genuinely overlong ID may break, without losing characters\n{text}"
            );
        }
    }
}
