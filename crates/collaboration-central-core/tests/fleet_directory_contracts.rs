//! The directory contracts follow the one-central-per-organization wire oracle.
use collaboration_central_core::identity_contract::{
    CentralAddresses, CentralLookup, CentralRecord, CreateOrganization, DirectoryCallAddresses,
    FindCentral, HttpsUrl, InitializeOrganization, OrganizationInitialization, ResourceUri,
    VerifiedEmail,
};
use schemars::{JsonSchema, generate::SchemaSettings};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;
#[path = "support/schema_validation.rs"]
mod schema_validation;

fn central_record_literal() -> Value {
    json!({
        "addresses": {
            "sessionEndpoint": "https://central-a.example.invalid/v1/session",
            "machineAddress": "https://central-a.example.invalid/machine",
            "adminAddress": "https://central-a.example.invalid/admin",
            "admissionAddress": "https://central-a.example.invalid/admission",
            "standingAddress": "https://central-a.example.invalid/standing",
            "machineTokenAddress": "https://central-a.example.invalid/machine-token",
            "receiver": "019a3dce-2000-7000-8000-000000000002",
            "relay": "019a3dce-2000-7000-8000-000000000003"
        },
        "directoryCalls": {
            "approvalAuthorizeAddress": "https://central-a.example.invalid/approval/authorize",
            "approvalActivateAddress": "https://central-a.example.invalid/approval/activate",
            "approvalCancelAddress": "https://central-a.example.invalid/approval/cancel",
            "initializationAddress": "https://central-a.example.invalid/initialize"
        }
    })
}

// Wrong behavior: the operator cannot supply the central that must initialize
// this organization. Oracle: Program Design "The new requests, exactly" and
// the by-hand runbook's step 3. This keeps the real serde boundary.
#[test]
fn fleet_directory_creation_carries_the_whole_central_record() {
    // Arrange
    let request = json!({
        "organizationId": "019a3dce-2000-7000-8000-000000000001",
        "ownerVerifiedEmail": "owner@example.invalid",
        "central": central_record_literal()
    });

    // Act
    let parsed = serde_json::from_value::<CreateOrganization>(request.clone());

    // Assert
    let parsed = parsed.expect("the operator supplies the whole CentralRecord");
    assert_eq!(serde_json::to_value(parsed).unwrap(), request);
    let mut missing_central = request;
    missing_central.as_object_mut().unwrap().remove("central");
    assert!(serde_json::from_value::<CreateOrganization>(missing_central).is_err());
}

// The shape names and literal tags come from PD c1f9471e. The real serde
// serializer and schema validator must reject unknown fields and wrong tags.
#[test]
fn fleet_directory_lookup_wire_contracts() {
    let addresses = central_record_literal()["addresses"].clone();
    contract_artifacts::assert_contract::<FindCentral>(
        "FindCentral",
        vec![json!({"organizationId":"019a3dce-2000-7000-8000-000000000001"})],
        false,
    );
    contract_artifacts::assert_contract::<CentralAddresses>(
        "CentralAddresses",
        vec![addresses.clone()],
        false,
    );
    contract_artifacts::assert_contract::<CentralLookup>(
        "CentralLookup",
        vec![
            json!({"result":"found","central":addresses}),
            json!({"result":"creating"}),
            json!({"result":"unknownOrganization"}),
        ],
        false,
    );
}

#[test]
fn fleet_directory_creation_wire_contracts() {
    contract_artifacts::assert_contract::<CentralRecord>(
        "CentralRecord",
        vec![central_record_literal()],
        false,
    );
    contract_artifacts::assert_contract::<DirectoryCallAddresses>(
        "DirectoryCallAddresses",
        vec![central_record_literal()["directoryCalls"].clone()],
        false,
    );
    contract_artifacts::assert_contract::<CreateOrganization>(
        "CreateOrganization",
        vec![json!({
            "organizationId":"019a3dce-2000-7000-8000-000000000001",
            "ownerVerifiedEmail":"owner@example.invalid",
            "central":central_record_literal()
        })],
        false,
    );
    contract_artifacts::assert_contract::<InitializeOrganization>(
        "InitializeOrganization",
        vec![json!({
            "organizationId":"019a3dce-2000-7000-8000-000000000001",
            "ownerEmail":"owner@example.invalid",
            "commandId":"019a3dce-2000-7000-8000-000000000004"
        })],
        false,
    );
    contract_artifacts::assert_contract::<OrganizationInitialization>(
        "OrganizationInitialization",
        vec![
            json!({"result":"initialized"}),
            json!({"result":"alreadyInitialized"}),
            json!({"result":"notThisCentral"}),
        ],
        false,
    );
}

// A public found answer cannot carry the private addresses even when its source
// is a complete CentralRecord. This also checks the external serde boundary.
#[test]
fn fleet_directory_public_lookup_excludes_directory_calls() {
    let record: CentralRecord = serde_json::from_value(central_record_literal()).unwrap();
    let answer = CentralLookup::Found {
        central: record.addresses,
    };
    let encoded = serde_json::to_value(answer).unwrap();
    assert_eq!(encoded["central"], central_record_literal()["addresses"]);
    assert!(encoded["central"].get("directoryCalls").is_none());

    let mut leaked = encoded;
    leaked["central"]["directoryCalls"] = central_record_literal()["directoryCalls"].clone();
    assert!(serde_json::from_value::<CentralLookup>(leaked).is_err());
}

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
    contract_artifacts::assert_contract::<VerifiedEmail>(
        "VerifiedEmail",
        vec![
            json!("owner@example.invalid"),
            json!("first.last+tag@example.invalid"),
            json!(format!("{}@example.invalid", "a".repeat(238))),
        ],
        false,
    );
}

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
        ("https://a:00080/a", true),
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

#[test]
fn fleet_directory_email_reads_normalize_after_validation() {
    assert_scalar_cases::<VerifiedEmail>(
        [
            ("owner@example.invalid", true),
            ("OWNER@EXAMPLE.INVALID", true),
            ("first.last+tag@example.invalid", true),
            (".owner@example.invalid", false),
            ("owner..name@example.invalid", false),
            ("owner@example.invalid\n", false),
            ("owner@example.invalid\r", false),
            ("öwner@example.invalid", false),
            ("owner@localhost", false),
            (" owner@example.invalid", false),
        ]
        .into_iter()
        .map(|(input, expected)| (input.to_owned(), expected))
        .chain([
            (format!("{}@example.invalid", "a".repeat(238)), true),
            (format!("{}@example.invalid", "a".repeat(239)), false),
        ])
        .collect(),
    );
    let email: VerifiedEmail = serde_json::from_value(json!("OWNER@EXAMPLE.INVALID")).unwrap();
    assert_eq!(email.as_str(), "owner@example.invalid");
    assert_eq!(
        serde_json::to_value(email).unwrap(),
        json!("owner@example.invalid")
    );
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
