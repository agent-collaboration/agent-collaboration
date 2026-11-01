use super::*;

#[test]
fn fitting_form_field_columns_keep_the_legacy_one_row_ansi_at_120_columns() {
    let current = element!(View(width:120,flex_direction:FlexDirection::Column) {FormFields {
        FormField(label:"Weekly".to_owned()) {Text(content:"42% remaining with a reset due tomorrow after another complete weekly window; this deliberately long value wraps inside the value column while the label stays beside its first line.",color:Color::White,wrap:TextWrap::Wrap)}
        FormField(label:"5h".to_owned()) {Text(content:"20% remaining",color:Color::White)}
    }})
    .render(None);
    let mut actual = Vec::new();
    current.write_ansi(&mut actual).unwrap();
    let baseline = include_bytes!("test_fixtures/form_field_legacy_120.ansi");
    assert_eq!(
        actual.as_slice(),
        baseline,
        "fitting columns remain byte-identical to legacy"
    );
}

#[test]
fn explicit_stacked_form_keeps_the_whole_label_above_the_value_at_32_columns() {
    let text=element!(View(width:32u16,flex_direction:FlexDirection::Column) {FormFields(stacked:true) {
        FormField(label:"Long section label 界".to_owned()) {crate::controls::fields::StatusValue(status:crate::controls::fields::StatusKind::Available,detail:Some("Local recording only".to_owned()))}
    }}).render(None).to_string();
    let label = text
        .lines()
        .position(|line| line.contains("Long section label 界"))
        .unwrap();
    let value = text
        .lines()
        .position(|line| line.contains("Available"))
        .unwrap();
    assert!(
        value > label,
        "complete value follows the complete label: {text}"
    );
}
