//! Decimal display projections never feed observations or routing decisions.
use codex_router_core::credit_usage::CreditBalance;

fn normalized_parts(balance: &CreditBalance) -> (&str, &str) {
    let raw = balance.as_str();
    let (integer, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    let integer = integer.trim_start_matches('0');
    (
        if integer.is_empty() { "0" } else { integer },
        fraction.trim_end_matches('0'),
    )
}

pub(super) fn detailed_credit_balance(balance: &CreditBalance) -> String {
    let (integer, fraction) = normalized_parts(balance);
    let mut output = String::new();
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(digit);
    }
    if !fraction.is_empty() {
        output.push('.');
        output.push_str(fraction);
    }
    output
}

pub(super) fn compact_credit_balance(balance: &CreditBalance) -> String {
    let (integer, fraction) = normalized_parts(balance);
    let all_digits = format!("{integer}{fraction}");
    let significant = all_digits.trim_start_matches('0');
    if significant.is_empty() {
        return "0".to_owned();
    }
    let mut exponent = if integer != "0" {
        i64::try_from(integer.len()).unwrap_or(i64::MAX) - 1
    } else {
        -(i64::try_from(fraction.len() - fraction.trim_start_matches('0').len())
            .unwrap_or(i64::MAX))
            - 1
    };
    // Four meaningful digits, rounded in base ten. No binary floating point.
    let mut digits = significant.bytes().take(4).collect::<Vec<_>>();
    if significant
        .as_bytes()
        .get(4)
        .is_some_and(|digit| *digit >= b'5')
    {
        let mut carry = true;
        for digit in digits.iter_mut().rev() {
            if *digit == b'9' {
                *digit = b'0';
            } else {
                *digit += 1;
                carry = false;
                break;
            }
        }
        if carry {
            digits.insert(0, b'1');
            digits.truncate(4);
            exponent += 1;
        }
    }
    let group = if exponent >= 3 { exponent / 3 } else { 0 };
    let decimal_position = exponent - group * 3 + 1;
    let mut output = String::new();
    if decimal_position <= 0 {
        output.push_str("0.");
        output.extend(std::iter::repeat_n(
            '0',
            usize::try_from(decimal_position.unsigned_abs()).unwrap_or(usize::MAX),
        ));
        output.extend(digits.iter().map(|digit| char::from(*digit)));
    } else {
        for (index, digit) in digits.iter().enumerate() {
            if index == usize::try_from(decimal_position).unwrap_or(usize::MAX) {
                output.push('.');
            }
            output.push(char::from(*digit));
        }
        if usize::try_from(decimal_position).unwrap_or(usize::MAX) > digits.len() {
            output.extend(std::iter::repeat_n(
                '0',
                usize::try_from(decimal_position).unwrap_or(usize::MAX) - digits.len(),
            ));
        }
    }
    if output.contains('.') {
        output = output
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned();
    }
    match group {
        0 => {}
        1 => output.push('k'),
        2 => output.push('m'),
        3 => output.push('b'),
        4 => output.push('t'),
        _ => output.push_str(&format!("e{}", group * 3)),
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_examples_preserve_exact_details_and_bound_compact_precision() {
        for (raw, compact, detailed) in [
            ("62500.0000000000000000", "62.5k", "62,500"),
            ("1234.50", "1.235k", "1,234.5"),
            ("0.000123456789", "0.0001235", "0.000123456789"),
            ("0.00000001", "0.00000001", "0.00000001"),
            ("999.94", "999.9", "999.94"),
            ("999.95", "1k", "999.95"),
            ("999999", "1m", "999,999"),
            ("1000000", "1m", "1,000,000"),
            ("0.0000", "0", "0"),
            ("00012.3400", "12.34", "12.34"),
            (
                "12345678901234567890.1250",
                "12.35e18",
                "12,345,678,901,234,567,890.125",
            ),
        ] {
            let balance = CreditBalance::new(raw).unwrap();
            assert_eq!(compact_credit_balance(&balance), compact, "{raw}");
            assert_eq!(detailed_credit_balance(&balance), detailed, "{raw}");
            assert_eq!(balance.as_str(), raw);
        }
        for invalid in ["-1", "NaN", "1e4", ""] {
            assert!(CreditBalance::new(invalid).is_err());
        }
    }

    use crate::presentation::quota::quota_account_options::credit_usage_compact_summary;
    use crate::presentation::quota::quota_browse_presentation_test::quota_two_account_view_model;
    use crate::quota::CreditUsageFreshness;
    use codex_router_core::credit_usage::CreditAvailability;
    #[test]
    fn compact_credit_summary_formats_exact_decimal_without_changing_observation() {
        use codex_router_core::credit_usage::{
            CreditBalance, CreditProviderObservation, CreditSpendControl,
        };
        let mut usage = quota_two_account_view_model().rows[0].credit_usage.clone();
        usage.freshness = CreditUsageFreshness::Fresh;
        usage.provider_observation = CreditProviderObservation::new(
            CreditAvailability::Available {
                balance: Some(CreditBalance::new("62500.0000000000000000").unwrap()),
            },
            CreditSpendControl::Clear,
            None,
        );
        let original = usage.clone();
        assert_eq!(credit_usage_compact_summary(&usage), "62.5k fresh");
        assert_eq!(
            usage, original,
            "presentation cannot change provider precision"
        );
    }
}
