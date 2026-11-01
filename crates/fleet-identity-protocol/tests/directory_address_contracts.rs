//! Preserved directory URL contracts, now owned by the shared protocol.
use fleet_identity_protocol::{HttpsUrl, ResourceUri};
use schemars::{JsonSchema, generate::SchemaSettings};
use serde::de::DeserializeOwned;
use serde_json::json;
#[path = "support/contract_artifacts.rs"]
#[cfg(test)]
mod contract_artifacts;
#[cfg(test)]
use contract_artifacts::schema_validation;

#[test]
fn fleet_directory_scalar_wire_contracts() {
    let addresses = vec![
        json!("https://central.example.invalid/v1/session"),
        json!("http://localhost:8787/"),
        json!("http://127.0.0.1:65535"),
        json!("http://[::1]:1/"),
        json!("https://xn--bcher-kva.example.invalid/a%20b"),
        json!(format!("https://a/{}", "a".repeat(2038))),
    ];
    contract_artifacts::assert_contract::<HttpsUrl>("HttpsUrl", addresses.clone(), false);
    contract_artifacts::assert_contract::<ResourceUri>("ResourceUri", addresses, false);
}

#[cfg(test)]
fn assert_scalar_cases<T: JsonSchema + DeserializeOwned>(cases: Vec<(String, bool)>) {
    let schema = serde_json::to_value(
        SchemaSettings::draft2020_12()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<T>(),
    )
    .unwrap();
    let validator = schema_validation::contract_validator(&schema).unwrap();
    for (input, expected) in cases {
        let value = json!(input);
        assert_eq!(
            serde_json::from_value::<T>(value.clone()).is_ok(),
            expected,
            "serde: {value}"
        );
        assert_eq!(validator.is_valid(&value), expected, "schema: {value}");
    }
}

// Wrong behaviors: widened URL-parser acceptance, bad port/label boundaries,
// byte truncation, or schema acceptance beyond serde. The oracle is the PD's
// explicit lexical rule, exercised through both real validators.
#[test]
fn fleet_directory_address_boundaries_match_schema_and_serde() {
    let mut cases = vec![
        ("https://a", true),
        ("http://localhost", true),
        ("http://127.0.0.1", true),
        ("http://[::1]", true),
        ("http://central.example.invalid", false),
        ("https://a:0", false),
        ("https://a:1", true),
        ("https://a:65535", true),
        ("https://a:65536", false),
        ("https://a:00080/a", false),
        ("https://a:0443", false),
        ("https://a:", false),
        ("https://-a.example.invalid", false),
        ("https://a-.example.invalid", false),
        ("https://UPPER.example.invalid", false),
        ("https://xn--bcher-kva.example.invalid", true),
        ("https://bücher.example.invalid", false),
        ("https://a/%20%2f%2F", true),
        ("https://a/%zz", false),
        ("https://a/%", false),
        ("https://a?query", false),
        ("https://a#fragment", false),
        ("https://user@a/", false),
        ("https://a/\n", false),
        ("https://a/\0", false),
        ("https://a/a b", false),
        ("https://a/a[b]", false),
    ]
    .into_iter()
    .map(|(input, expected)| (input.to_owned(), expected))
    .collect::<Vec<_>>();
    cases.extend([
        (format!("https://{}.invalid", "a".repeat(63)), true),
        (format!("https://{}.invalid", "a".repeat(64)), false),
        (format!("https://a/{}", "a".repeat(2038)), true),
        (format!("https://a/{}", "a".repeat(2039)), false),
    ]);
    assert_scalar_cases::<HttpsUrl>(cases.clone());
    assert_scalar_cases::<ResourceUri>(cases);
}

// Oracle correction 0f0c9a55: port spellings are canonical at the boundary.
#[test]
fn fleet_directory_refuses_leading_zero_ports() {
    for address in ["https://a:0443", "http://localhost:00080/a"] {
        assert!(
            HttpsUrl::try_from(address.to_owned()).is_err(),
            "leading-zero port accepted: {address}"
        );
        assert!(ResourceUri::try_from(address.to_owned()).is_err());
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config {
        cases: 128,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0xf2c),
        ..proptest::test_runner::Config::default()
    })]

    // The numeric oracle is independent of the lexical implementation:
    // every generated decimal port is admitted exactly within the PD's range.
    #[test]
    fn fleet_directory_port_range_matches_the_numeric_rule(
        host in "[a-z]{1,20}",
        path in "[a-z0-9/]{0,128}",
        port in 0_u32..70_000,
    ) {
        let address = format!("https://{host}:{port}/{path}");
        let expected = (1..=65535).contains(&port);
        assert_scalar_cases::<HttpsUrl>(vec![(address.clone(), expected)]);
        if expected {
            let parsed = HttpsUrl::try_from(address.clone()).unwrap();
            proptest::prop_assert_eq!(parsed.as_str(), address.as_str());
        }
    }
}
