//! The result tour must retain the intermediate consumed-credit fact.
use super::super::vhs_wait_pattern_oracle::{right_padded_rows, wait_pattern};
use super::*;

#[test]
fn normal_result_tour_captures_consumption_before_final_guidance() {
    for grid in [(32, 24), (48, 12), (40, 24), (48, 24), (76, 30), (120, 36)] {
        let tape = build_normal_component_tape(grid.0, grid.1).expect("declared grid");
        let top = "Screenshot component-quota-reset-result.png";
        let middle = "Screenshot component-quota-reset-result-consumption.png";
        let bottom = "Screenshot component-quota-reset-result-bottom.png";
        let top_index = tape.find(top).expect("success pose");
        let middle_index = tape.find(middle).expect("consumption pose");
        let bottom_index = tape.find(bottom).expect("guidance pose");
        assert!(top_index < middle_index && middle_index < bottom_index);
        assert_eq!(tape.matches(middle).count(), 1);
        let between = tape
            .get(top_index..middle_index)
            .expect("ordered checkpoints");
        assert_eq!(
            between.lines().filter(|line| *line == "Down").count(),
            usize::from(grid == (48, 12)) * 2,
            "measured middle pose at {grid:?}"
        );
        let missing = NORMAL_CHECKPOINTS
            .into_iter()
            .filter(|checkpoint| *checkpoint != "component-quota-reset-result-consumption")
            .collect::<Vec<_>>();
        assert!(validate_normal_checkpoints(&missing).is_err());
    }
}

#[test]
fn consumption_readiness_requires_the_complete_positive_sentence() {
    let tape = build_normal_component_tape(48, 12).expect("declared short grid");
    let prefix = tape
        .split("Screenshot component-quota-reset-result-consumption.png")
        .next()
        .expect("middle pose");
    let wait = prefix
        .lines()
        .rfind(|line| line.starts_with("Wait+Screen /"))
        .expect("middle readiness");
    let expression = wait
        .strip_prefix("Wait+Screen /")
        .unwrap()
        .strip_suffix('/')
        .unwrap();
    let pattern = wait_pattern(expression).expect("actual tape expression");
    let positive = "Reset result [alpha-tag]\n│ One reset credit was consumed. │";
    assert!(pattern.is_match(positive));
    assert!(pattern.is_match(&right_padded_rows(positive, 200)));
    assert!(pattern.is_match("Reset result [alpha-tag]\n│ One reset credit │\n│ was consumed. │"));
    for sentence in [
        "No reset credit was consumed.",
        "One reset credit was not consumed.",
        "One reset credit was consumed",
        "One reset credit was used.",
    ] {
        assert!(
            !pattern.is_match(&format!("Reset result [alpha-tag] {sentence}")),
            "{sentence}"
        );
    }
}
