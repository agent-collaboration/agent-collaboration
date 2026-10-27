//! Rust `regex` oracle for generated VHS Wait patterns over retained screen text.
//!
//! VHS evaluates `Wait+Screen` with Go's RE2 engine. Rust `regex` is the same
//! linear-time syntax family (no backreferences or lookaround), so each
//! positive and negative screen exercises the generated pattern text itself.
use regex::Regex;
use regex::RegexBuilder;

use super::TestResult;

/// Compile one generated Wait pattern with room for its bounded `{1,1000}` repeats.
pub(super) fn wait_pattern(pattern: &str) -> TestResult<Regex> {
    Ok(RegexBuilder::new(pattern).size_limit(1 << 28).build()?)
}

/// Right-pad each retained row to `width` characters, preserving the original test vectors.
pub(super) fn right_padded_rows(screen: &str, width: usize) -> String {
    screen
        .lines()
        .map(|row| format!("{row:<width$}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every positive screen satisfies the wait and no negative screen does.
pub(super) fn ensure_wait_shapes(
    pattern: &Regex,
    positives: &[String],
    negatives: &[String],
) -> TestResult<()> {
    for (index, screen) in positives.iter().enumerate() {
        if !pattern.is_match(screen) {
            return Err(format!("positive screen {index} must satisfy the wait").into());
        }
    }
    for (index, screen) in negatives.iter().enumerate() {
        if pattern.is_match(screen) {
            return Err(format!("negative screen {index} must not satisfy the wait").into());
        }
    }
    Ok(())
}
