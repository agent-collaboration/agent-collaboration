use super::SignInTrustSettings;

#[test]
fn machine_audience_preserves_the_deployments_exact_d08_address() {
    let mut settings = SignInTrustSettings {
        issuer: "https://signin.example.invalid".into(),
        audience: "identity".into(),
        admin_audience: "admin".into(),
        machine_audience: String::new(),
        public_origin: "https://central.example.invalid".into(),
        jwks_url: "https://signin.example.invalid/api/auth/jwks".into(),
        service_id: None,
    };
    for address in [
        "https://central.example.invalid/machine",
        "https://central.example.invalid:443/a%2fb",
        "http://localhost:8787/machine",
        "http://127.0.0.1/",
        "http://[::1]:8787/machine",
    ] {
        settings.machine_audience = address.into();
        assert!(settings.machine_audience_is_valid(), "{address}");
        assert_eq!(settings.machine_audience, address);
    }
    for address in [
        "",
        "http://central.example.invalid/machine",
        "https://USER@central.example.invalid/",
        "https://UPPER.invalid/",
        "https://central.invalid/?query=x",
        "https://central.invalid/#fragment",
        "https://central.invalid/a%2",
        "https://central.invalid:0/machine",
        "https://central.invalid:65536/",
        "https://central.invalid:0443/",
    ] {
        settings.machine_audience = address.into();
        assert!(!settings.machine_audience_is_valid(), "{address}");
    }
}
