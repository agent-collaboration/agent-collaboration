use sqlx_core::connection::ConnectOptions;

use crate::DoConnectOptions;

#[test]
fn parses_a_do_url() {
    let options: DoConnectOptions = "do://".parse().unwrap();
    assert_eq!(options.to_url_lossy().as_str(), "do://");
}

#[test]
fn rejects_other_schemes() {
    assert!("sqlite://db".parse::<DoConnectOptions>().is_err());
    assert!("d1://DB".parse::<DoConnectOptions>().is_err());
}
