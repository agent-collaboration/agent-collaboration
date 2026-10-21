//! Observation-field synchronization across retained background screen text.
use super::*;

#[test]
fn observation_wait_keeps_exact_field_tokens_across_retained_background() -> TestResult<()> {
    let script = r#"import re, sys
pattern = re.compile(sys.argv[1], re.S)
# Exact separator retained from failed76 full-screen Wait value; no pixels made.
separator = '                                         │ │      │ │                                                                                                                                                                                                                                                                                             \n│ │    Cla│ '
medium = 'Options · Credits [alpha-tag] Observation' + separator + 'checked 0s ago │'
valid = [medium, 'Options · Credits [alpha-tag] Observation checked 0s ago', medium.replace('\n', '\r\n')]
assert all(pattern.search(value) for value in valid), 'retained76 background prefix must synchronize'
invalid = [
    medium.replace('checked 0s ago', 'checked 10s ago'),
    medium.replace('checked 0s ago', 'not checked 0s ago'),
    medium.replace('Options · Credits', 'Credit usage'),
    medium.replace('alpha-tag', 'beta-tag'),
    medium.replace('Observation', 'Freshness'),
    medium.replace('checked 0s ago', 'checked0sago'),
    medium.replace('checked 0s ago', 'checked 0s'),
    medium.replace('checked 0s ago', '0s ago checked'),
    medium.replace('checked 0s ago', 'checked 0s agoX'),
]
assert not any(pattern.search(value) for value in invalid), 'exact pane/tag/label/order/age/word boundaries must reject'
print('3positive/9negative observation readiness shapes')
"#;
    let pattern = format!(
        "(?s){}",
        credit_field_readiness("Observation", r"checked[\s│┃]{1,1000}0s[\s│┃]{1,1000}ago")
    );
    let result = std::process::Command::new("/usr/bin/python3")
        .args(["-c", script, &pattern])
        .output()?;
    if !result.status.success() {
        return Err(
            std::io::Error::other(String::from_utf8_lossy(&result.stderr).into_owned()).into(),
        );
    }
    Ok(())
}

#[test]
fn freshness_wait_keeps_exact_field_tokens_across_retained_background() -> TestResult<()> {
    let script = r#"import re, sys
pattern = re.compile(sys.argv[1], re.S)
separator = '                                           │ │      │ │                                                                                                                                                                                                                 \n│ │    Cla│ '
medium = 'Options · Credits [alpha-tag] Freshness' + separator + 'Fresh │'
valid = [medium, 'Options · Credits [alpha-tag] Freshness Fresh', medium.replace('\n', '\r\n')]
assert all(pattern.search(value) for value in valid), 'retained76 Freshness background prefix must synchronize'
invalid = [
    medium.replace('│ Fresh │', '│ Stale │'),
    medium.replace('│ Fresh │', '│ not Fresh │'),
    medium.replace('│ Fresh │', '│ Freshly │'),
    medium.replace('│ Fresh │', '│ checked 10s ago │'),
    medium.replace('Freshness', 'Observation'),
    medium.replace('Freshness', 'NotFreshness'),
    medium.replace('alpha-tag', 'beta-tag'),
    medium.replace('Options · Credits', 'Credit usage'),
    'Options · Credits [alpha-tag] Fresh Freshness',
    'Options · Credits [alpha-tag] Freshness foreground Fresh',
]
assert not any(pattern.search(value) for value in invalid), 'exact pane/tag/field/value/order/boundaries must reject'
print('3positive/10negative freshness readiness shapes')
"#;
    let pattern = format!("(?s){}", credit_field_readiness("Freshness", "Fresh"));
    let result = std::process::Command::new("/usr/bin/python3")
        .args(["-c", script, &pattern])
        .output()?;
    if !result.status.success() {
        return Err(
            std::io::Error::other(String::from_utf8_lossy(&result.stderr).into_owned()).into(),
        );
    }
    Ok(())
}
