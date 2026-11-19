use super::ServeConfiguration;
use crate::central_command::root_lock::lock_root;
use std::os::unix::fs::PermissionsExt;

const CONFIG: &str = r#"{
    // Public link bind, independent of the issuer's audience.
    "listen": "127.0.0.1:0",
    "root": "/tmp/central-fixture",
    "signIn": {
        "issuer": "https://signin.example.invalid",
        "audience": "central-stamp",
        "adminAudience": "central-admin",
        "machineAudience": "http://127.0.0.1:49100",
        "publicOrigin": "https://central.example.invalid",
        "jwksUrl": "https://signin.example.invalid/jwks",
        "serviceId": "019f0000-0000-7000-8000-000000000001",
    },
}"#;

#[test]
fn central_serve_reads_only_the_named_jsonc_fields() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("central.jsonc");
    std::fs::write(&path, CONFIG).unwrap();

    let config = ServeConfiguration::read(&path).unwrap();
    assert_eq!(config.listen, "127.0.0.1:0".parse().unwrap());
    assert_eq!(config.root, std::path::Path::new("/tmp/central-fixture"));
    assert_eq!(config.sign_in.machine_audience, "http://127.0.0.1:49100");

    for invalid in [
        CONFIG.replace("\"root\":", "\"root\": \"/tmp/other\", \"root\":"),
        CONFIG.replace(
            "\"root\":",
            "\"publicUrl\": \"https://example.invalid\", \"root\":",
        ),
        CONFIG.replace(
            "\"jwksUrl\":",
            "\"unknownIssuerSetting\": true, \"jwksUrl\":",
        ),
    ] {
        std::fs::write(&path, invalid).unwrap();
        assert!(ServeConfiguration::read(&path).is_err());
    }
}

#[test]
fn a_serve_root_is_private_and_exclusively_owned() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("central");
    let lock = lock_root(&root).unwrap();

    assert_eq!(
        std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(lock_root(&root).is_err());

    drop(lock);
    std::fs::set_permissions(&root, PermissionsExt::from_mode(0o750)).unwrap();
    assert!(lock_root(&root).is_err());
}
