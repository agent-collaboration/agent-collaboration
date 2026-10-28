//! Observation-field synchronization across retained background screen text.
use super::super::vhs_wait_pattern_oracle::{ensure_wait_shapes, wait_pattern};
use super::*;

#[test]
fn observation_wait_keeps_exact_field_tokens_across_retained_background() -> TestResult<()> {
    let pattern = wait_pattern(&format!(
        "(?s){}",
        credit_field_readiness("Observation", r"checked[\s│┃]{1,1000}0s[\s│┃]{1,1000}ago")
    ))?;
    // Exact separator retained from failed76 full-screen Wait value; no pixels made.
    let separator = "                                         │ │      │ │                                                                                                                                                                                                                                                                                             \n│ │    Cla│ ";
    let medium = format!("Options · Credits [alpha-tag] Observation{separator}checked 0s ago │");
    // Retained76 background prefix must synchronize.
    let valid = [
        medium.clone(),
        "Options · Credits [alpha-tag] Observation checked 0s ago".to_owned(),
        medium.replace('\n', "\r\n"),
    ];
    // Exact pane/tag/label/order/age/word boundaries must reject.
    let invalid = [
        medium.replace("checked 0s ago", "checked 10s ago"),
        medium.replace("checked 0s ago", "not checked 0s ago"),
        medium.replace("Options · Credits", "Credit usage"),
        medium.replace("alpha-tag", "beta-tag"),
        medium.replace("Observation", "Freshness"),
        medium.replace("checked 0s ago", "checked0sago"),
        medium.replace("checked 0s ago", "checked 0s"),
        medium.replace("checked 0s ago", "0s ago checked"),
        medium.replace("checked 0s ago", "checked 0s agoX"),
    ];
    ensure_wait_shapes(&pattern, &valid, &invalid)
}

#[test]
fn freshness_wait_keeps_exact_field_tokens_across_retained_background() -> TestResult<()> {
    let pattern = wait_pattern(&format!(
        "(?s){}",
        credit_field_readiness("Freshness", "Fresh")
    ))?;
    let separator = "                                           │ │      │ │                                                                                                                                                                                                                 \n│ │    Cla│ ";
    let medium = format!("Options · Credits [alpha-tag] Freshness{separator}Fresh │");
    // Retained76 Freshness background prefix must synchronize.
    let valid = [
        medium.clone(),
        "Options · Credits [alpha-tag] Freshness Fresh".to_owned(),
        medium.replace('\n', "\r\n"),
    ];
    // Exact pane/tag/field/value/order/boundaries must reject.
    let invalid = [
        medium.replace("│ Fresh │", "│ Stale │"),
        medium.replace("│ Fresh │", "│ not Fresh │"),
        medium.replace("│ Fresh │", "│ Freshly │"),
        medium.replace("│ Fresh │", "│ checked 10s ago │"),
        medium.replace("Freshness", "Observation"),
        medium.replace("Freshness", "NotFreshness"),
        medium.replace("alpha-tag", "beta-tag"),
        medium.replace("Options · Credits", "Credit usage"),
        "Options · Credits [alpha-tag] Fresh Freshness".to_owned(),
        "Options · Credits [alpha-tag] Freshness foreground Fresh".to_owned(),
    ];
    ensure_wait_shapes(&pattern, &valid, &invalid)
}
