use super::*;

#[test]
fn the_store_lives_at_provider_proxy_sqlite_and_was_state_sqlite() {
    let root = Path::new("/router-root");

    assert_eq!(
        provider_proxy_database_path(root),
        PathBuf::from("/router-root/provider-proxy.sqlite")
    );
    assert_eq!(
        former_provider_proxy_database_path(root),
        PathBuf::from("/router-root/state.sqlite")
    );
}
