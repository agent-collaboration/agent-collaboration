use super::*;

#[test]
fn the_store_lives_at_provider_proxy_sqlite() {
    let root = Path::new("/router-root");

    assert_eq!(
        provider_proxy_database_path(root),
        PathBuf::from("/router-root/provider-proxy.sqlite")
    );
}
