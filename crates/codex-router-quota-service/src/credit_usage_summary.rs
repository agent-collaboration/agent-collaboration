//! Pure decimal quota display projections.
use crate::quota::{CreditUsageFreshness, CreditUsageStatus};
use codex_router_core::credit_usage::{CreditAvailability, CreditBalance};
pub fn normalized_parts(balance: &CreditBalance) -> (&str, &str) {
    let raw = balance.as_str();
    let (integer, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    let integer = integer.trim_start_matches('0');
    (
        if integer.is_empty() { "0" } else { integer },
        fraction.trim_end_matches('0'),
    )
}

pub fn compact_credit_balance(balance: &CreditBalance) -> String {
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

pub fn credit_usage_compact_summary(usage: &CreditUsageStatus) -> String {
    let freshness = match usage.freshness {
        CreditUsageFreshness::Fresh => "fresh",
        CreditUsageFreshness::Stale => "stale",
        CreditUsageFreshness::Unknown => "unknown",
    };
    match usage.provider_observation.availability() {
        CreditAvailability::Unknown => "unknown".to_owned(),
        CreditAvailability::Depleted => format!("depleted {freshness}"),
        CreditAvailability::Unlimited => format!("unlimited {freshness}"),
        CreditAvailability::Available {
            balance: Some(balance),
        } => format!("{} {freshness}", compact_credit_balance(balance)),
        CreditAvailability::Available { balance: None } => format!("available {freshness}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_router_core::credit_usage::CreditAvailability;
    #[test]
    fn compact_credit_summary_formats_exact_decimal_without_changing_observation() {
        use codex_router_core::credit_usage::{
            CreditBalance, CreditProviderObservation, CreditSpendControl,
        };
        let usage = CreditUsageStatus {
            freshness: CreditUsageFreshness::Fresh,
            provider_observation: CreditProviderObservation::new(
                CreditAvailability::Available {
                    balance: Some(CreditBalance::new("62500.0000000000000000").unwrap()),
                },
                CreditSpendControl::Clear,
                None,
            ),
            ..CreditUsageStatus::default()
        };
        let original = usage.clone();
        assert_eq!(credit_usage_compact_summary(&usage), "62.5k fresh");
        assert_eq!(
            usage, original,
            "presentation cannot change provider precision"
        );
    }
}
