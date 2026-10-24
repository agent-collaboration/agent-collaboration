//! Stored address, Worker name and native file key remain one stable physical identity.
#![allow(clippy::panic_in_result_fn)]
#[path = "../src/object_keys.rs"]
mod native_keys;
#[path = "../../collaboration-central-cloudflare/src/object_location.rs"]
mod worker_location;
use collaboration_central_core::ObjectAddress;

#[test]
fn copy_identity_keeps_the_original_json_worker_name_and_native_key()
-> Result<(), Box<dyn std::error::Error>> {
    let original = br#"{"kind":"copy","value":"019f0000-0000-7000-8000-000000000001"}"#;
    let address: ObjectAddress = serde_json::from_slice(original)?;
    assert_eq!(serde_json::to_vec(&address)?, original);
    let (namespace, name) = worker_location::location(&address);
    assert_eq!(namespace, "PROJECT_COPIES");
    assert_eq!(name, "019f0000-0000-7000-8000-000000000001");
    assert_eq!(
        native_keys::address_key(&address)?,
        "copy-019f0000-0000-7000-8000-000000000001"
    );
    assert_eq!(
        native_keys::address_from_key("copy-019f0000-0000-7000-8000-000000000001")?,
        address
    );
    Ok(())
}
