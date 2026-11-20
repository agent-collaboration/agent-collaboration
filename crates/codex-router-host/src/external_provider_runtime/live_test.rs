use super::*;
use collaboration_protocol::UuidIdentity;

#[test]
fn provider_output_classifier_reports_only_bounded_uuid_metadata() {
    let expected = UuidIdentity::try_from("0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89".to_owned())
        .expect("expected service ID");
    let different = "2ff962c5-7fa3-4c18-a5ca-1bbe8db09e81";
    let cases = [
        (
            String::from(expected.clone()),
            ProviderOutputClassification {
                output_empty: false,
                exact_identity_equal: true,
                expected_identity_token_present: true,
                returned_uuid_count: 1,
                sole_uuid_matches_expected: true,
                output_byte_count: 36,
            },
        ),
        (
            format!("`{}`", String::from(expected.clone())),
            ProviderOutputClassification {
                output_empty: false,
                exact_identity_equal: false,
                expected_identity_token_present: true,
                returned_uuid_count: 1,
                sole_uuid_matches_expected: true,
                output_byte_count: 38,
            },
        ),
        (
            format!("service id: {}", String::from(expected.clone())),
            ProviderOutputClassification {
                output_empty: false,
                exact_identity_equal: false,
                expected_identity_token_present: true,
                returned_uuid_count: 1,
                sole_uuid_matches_expected: true,
                output_byte_count: 48,
            },
        ),
        (
            different.to_owned(),
            ProviderOutputClassification {
                output_empty: false,
                exact_identity_equal: false,
                expected_identity_token_present: false,
                returned_uuid_count: 1,
                sole_uuid_matches_expected: false,
                output_byte_count: 36,
            },
        ),
        (
            String::new(),
            ProviderOutputClassification {
                output_empty: true,
                exact_identity_equal: false,
                expected_identity_token_present: false,
                returned_uuid_count: 0,
                sole_uuid_matches_expected: false,
                output_byte_count: 0,
            },
        ),
        (
            "no identity".to_owned(),
            ProviderOutputClassification {
                output_empty: false,
                exact_identity_equal: false,
                expected_identity_token_present: false,
                returned_uuid_count: 0,
                sole_uuid_matches_expected: false,
                output_byte_count: 11,
            },
        ),
        (
            format!("{} {different}", String::from(expected.clone())),
            ProviderOutputClassification {
                output_empty: false,
                exact_identity_equal: false,
                expected_identity_token_present: true,
                returned_uuid_count: 2,
                sole_uuid_matches_expected: false,
                output_byte_count: 73,
            },
        ),
    ];
    for (output, expected_classification) in cases {
        assert_eq!(
            classify_provider_output(&output, &expected),
            expected_classification
        );
    }
    let expected_text = String::from(expected.clone());
    for embedded in [
        format!("x{expected_text}"),
        format!("{expected_text}x"),
        format!("_{expected_text}"),
        format!("{expected_text}_"),
        format!("-{expected_text}"),
        format!("{expected_text}-"),
        format!("0{expected_text}0"),
    ] {
        let classification = classify_provider_output(&embedded, &expected);
        assert_eq!(classification.returned_uuid_count, 0);
        assert!(!classification.expected_identity_token_present);
        assert!(!classification.sole_uuid_matches_expected);
    }
}
