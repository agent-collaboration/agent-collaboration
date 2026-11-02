use super::*;
use crate::dialog::{DialogSection, DialogSections};

#[test]
fn separate_form_sections_share_one_dialog_wide_value_origin() {
    let minimum = FormFields::label_width_for(["ID", "マシン設定"]);
    let frame=element!(View(width:60,flex_direction:FlexDirection::Column) {
            DialogSections {
                DialogSection(heading:"Identity".to_owned()) {
                    FormFields(minimum_label_width:Some(minimum)) {FormField(label:"ID".to_owned()) {Text(content:"FIRST_VALUE")}}
                }
                DialogSection(heading:"Connection".to_owned()) {
                    FormFields(minimum_label_width:Some(minimum)) {FormField(label:"マシン設定".to_owned()) {Text(content:"SECOND_VALUE")}}
                }
            }
        }).render(None).to_string();
    for value in ["FIRST_VALUE", "SECOND_VALUE"] {
        let prefix = frame
            .lines()
            .find(|line| line.contains(value))
            .unwrap()
            .split_once(value)
            .unwrap()
            .0;
        assert_eq!(UnicodeWidthStr::width(prefix), 14, "{value}: {frame}");
    }
}

#[test]
fn minimum_label_width_preserves_local_measurement_and_default_bytes() {
    assert_eq!(FormFields::label_width_for(["ID", "マシン設定"]), 10);
    assert_eq!(FormFields::label_width_for(std::iter::empty::<&str>()), 0);
    let mut baseline = None;
    for minimum in [None, Some(0), Some(1)] {
        let canvas = element!(View(width:40) {FormFields(minimum_label_width:minimum) {
            FormField(label:"マシン設定".to_owned()) {Text(content:"VALUE")}
        }})
        .render(None);
        let mut bytes = Vec::new();
        canvas.write_ansi(&mut bytes).unwrap();
        if let Some(expected) = &baseline {
            assert_eq!(&bytes, expected);
        } else {
            baseline = Some(bytes);
        }
        let frame = canvas.to_string();
        let prefix = frame
            .lines()
            .find(|line| line.contains("VALUE"))
            .unwrap()
            .split_once("VALUE")
            .unwrap()
            .0;
        assert_eq!(UnicodeWidthStr::width(prefix), 14, "{frame}");
    }
}

#[test]
fn measured_unicode_labels_and_focus_share_one_value_origin() {
    for stacked in [false, true] {
        for selected in [false, true] {
            let frame = element!(View(width:40) {
                    FormFields(stacked) {
                        FormField(label:"マシン設定".to_owned(),has_focus:selected) {Text(content:"Sunbook")}
                        FormField(label:"Folder".to_owned()) {Text(content:"/repo")}
                    }
                }).render(None).to_string();
            for value in ["Sunbook", "/repo"] {
                let prefix = frame
                    .lines()
                    .find(|line| line.contains(value))
                    .unwrap()
                    .split_once(value)
                    .unwrap()
                    .0;
                assert_eq!(
                    UnicodeWidthStr::width(prefix),
                    if stacked { 2 } else { 14 },
                    "{frame}"
                );
            }
            assert_eq!(frame.matches('❯').count(), usize::from(selected), "{frame}");
        }
    }
}
