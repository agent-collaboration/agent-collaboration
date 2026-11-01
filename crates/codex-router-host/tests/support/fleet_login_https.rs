//! Real HTTPS for the isolated C8 stand-in, with fixed certificates and injected TLS time.
use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, UnixTime},
    },
};

#[derive(Debug)]
struct CertificateTime;
impl rustls::time_provider::TimeProvider for CertificateTime {
    fn current_time(&self) -> Option<UnixTime> {
        Some(UnixTime::since_unix_epoch(Duration::from_secs(
            1_800_000_000,
        )))
    }
}

pub struct HttpsListener {
    tcp: TcpListener,
    tls: TlsAcceptor,
}
impl HttpsListener {
    pub async fn bind() -> Result<Self, Box<dyn std::error::Error>> {
        let certificate =
            CertificateDer::from(include_bytes!("../fixtures/fleet-login-tls/server.der").to_vec());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            include_bytes!("../fixtures/fleet-login-tls/server-key.der").to_vec(),
        ));
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)?;
        config.time_provider = Arc::new(CertificateTime);
        Ok(Self {
            tcp: TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?,
            tls: TlsAcceptor::from(Arc::new(config)),
        })
    }
    pub fn address(&self) -> io::Result<SocketAddr> {
        self.tcp.local_addr()
    }
}
impl axum::serve::Listener for HttpsListener {
    type Io = tokio_rustls::server::TlsStream<TcpStream>;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (stream, address) = match self.tcp.accept().await {
                Ok(accepted) => accepted,
                Err(_) => {
                    tracing::error!("F11 fixture TCP accept failed");
                    return std::future::pending().await;
                }
            };
            match self.tls.accept(stream).await {
                Ok(stream) => return (stream, address),
                // A client without fixture trust may reject this certificate.
                // That connection ends; the listener still serves its issuer.
                Err(_) => tracing::debug!("F11 fixture TLS connection rejected"),
            }
        }
    }
    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

pub fn issuer_client() -> Result<reqwest::Client, Box<dyn std::error::Error>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(
        include_bytes!("../fixtures/fleet-login-tls/root.der").to_vec(),
    ))?;
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.time_provider = Arc::new(CertificateTime);
    Ok(reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .pool_idle_timeout(None)
        .tcp_keepalive(None)
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
