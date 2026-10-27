//! Decimal display projections never feed observations or routing decisions.
use codex_router_core::credit_usage::CreditBalance;

use codex_router_quota_service::credit_usage_summary::normalized_parts;

pub(crate) fn detailed_credit_balance(balance: &CreditBalance) -> String {
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

#[cfg(test)]
use codex_router_quota_service::credit_usage_summary::compact_credit_balance;

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

    use crate::quota_browse_presentation_test::quota_two_account_view_model;
    use codex_router_core::credit_usage::CreditAvailability;
    use codex_router_quota_service::credit_usage_summary::credit_usage_compact_summary;
    use codex_router_quota_service::quota::CreditUsageFreshness;
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
