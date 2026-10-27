use super::*;
#[test]
fn debug_session_profile_selection_matches_router_default_boundary() {
    use codex_native_integration::SessionProfile;
    assert_eq!(
        session_profile_for_environment(true, false),
        SessionProfile::RouterDebug
    );
    assert_eq!(
        session_profile_for_environment(true, true),
        SessionProfile::Router
    );
    assert_eq!(
        session_profile_for_environment(false, false),
        SessionProfile::Router
    );
}

#[test]
fn codex_home_resolution_uses_real_home_without_debug_redirect() {
    let home = PathBuf::from("/tmp/codex-router-home-policy");
    let explicit_codex_home = PathBuf::from("/tmp/explicit-codex-home");

    assert_eq!(
        codex_home_from_environment(None, Some(home.clone()))
            .expect("HOME should resolve Codex home"),
        home.join(".codex")
    );
    assert_eq!(
        codex_home_from_environment(Some(explicit_codex_home.clone()), Some(home))
            .expect("CODEX_HOME should win"),
        explicit_codex_home
    );
}
