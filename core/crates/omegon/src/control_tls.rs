use std::fs::File;
use std::future::Future;
use std::io::{self, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::serve::Listener;
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlTlsConfig {
    pub cert_chain_path: PathBuf,
    pub private_key_path: PathBuf,
    pub client_ca_path: Option<PathBuf>,
}

impl ControlTlsConfig {
    pub fn is_mtls(&self) -> bool {
        self.client_ca_path.is_some()
    }
}

pub fn schemes(tls: Option<&ControlTlsConfig>) -> (&'static str, &'static str) {
    if tls.is_some() {
        ("https", "wss")
    } else {
        ("http", "ws")
    }
}

pub async fn serve_router(
    listener: TcpListener,
    app: Router,
    tls: Option<ControlTlsConfig>,
) -> anyhow::Result<()> {
    if let Some(tls) = tls {
        axum::serve(TlsListener::new(listener, tls)?, app)
            .await
            .map_err(anyhow::Error::from)
    } else {
        axum::serve(listener, app)
            .await
            .map_err(anyhow::Error::from)
    }
}

pub async fn serve_router_with_shutdown<F>(
    listener: TcpListener,
    app: Router,
    tls: Option<ControlTlsConfig>,
    signal: F,
) -> anyhow::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    if let Some(tls) = tls {
        axum::serve(TlsListener::new(listener, tls)?, app)
            .with_graceful_shutdown(signal)
            .await
            .map_err(anyhow::Error::from)
    } else {
        axum::serve(listener, app)
            .with_graceful_shutdown(signal)
            .await
            .map_err(anyhow::Error::from)
    }
}

struct TlsListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl TlsListener {
    fn new(listener: TcpListener, tls: ControlTlsConfig) -> io::Result<Self> {
        let server_config = build_tls_server_config(&tls)?;
        Ok(Self {
            listener,
            acceptor: TlsAcceptor::from(server_config),
        })
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (stream, addr) = match self.listener.accept().await {
                Ok(pair) => pair,
                Err(err) => {
                    tracing::error!(error = %err, "control-plane TCP accept failed");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            match self.acceptor.accept(stream).await {
                Ok(tls_stream) => return (tls_stream, addr),
                Err(err) => {
                    tracing::warn!(peer = %addr, error = %err, "control-plane TLS handshake failed");
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

fn build_tls_server_config(config: &ControlTlsConfig) -> io::Result<Arc<ServerConfig>> {
    let server_chain = load_cert_chain(config.cert_chain_path.as_path())?;
    let private_key = load_private_key(config.private_key_path.as_path())?;

    let builder = ServerConfig::builder();
    let server_config = if let Some(client_ca_path) = config.client_ca_path.as_ref() {
        let roots = load_root_store(client_ca_path.as_path())?;
        let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "failed to build client verifier from {}: {}",
                        client_ca_path.display(),
                        err
                    ),
                )
            })?;
        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(server_chain, private_key)
            .map_err(invalid_server_cert)?
    } else {
        builder
            .with_no_client_auth()
            .with_single_cert(server_chain, private_key)
            .map_err(invalid_server_cert)?
    };

    Ok(Arc::new(server_config))
}

fn invalid_server_cert(err: rustls::Error) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid control-plane TLS server certificate/key configuration: {err}"),
    )
}

fn load_cert_chain(path: &Path) -> io::Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let certificates = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to parse PEM certs from {}: {}", path.display(), err),
            )
        })?;
    if certificates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no certificates found in {}", path.display()),
        ));
    }
    Ok(certificates)
}

fn load_private_key(path: &Path) -> io::Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let key = rustls_pemfile::private_key(&mut reader).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse private key {}: {}", path.display(), err),
        )
    })?;
    key.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no private key found in {}", path.display()),
        )
    })
}

fn load_root_store(path: &Path) -> io::Result<RootCertStore> {
    let certificates = load_cert_chain(path)?;
    let mut roots = RootCertStore::empty();
    let (added, _ignored) = roots.add_parsable_certificates(certificates);
    if added == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no valid CA certificates found in {}", path.display()),
        ));
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_follow_tls_presence() {
        assert_eq!(schemes(None), ("http", "ws"));
        let tls = ControlTlsConfig {
            cert_chain_path: "cert.pem".into(),
            private_key_path: "key.pem".into(),
            client_ca_path: None,
        };
        assert_eq!(schemes(Some(&tls)), ("https", "wss"));
    }

    #[test]
    fn mtls_detects_client_ca() {
        let tls = ControlTlsConfig {
            cert_chain_path: "cert.pem".into(),
            private_key_path: "key.pem".into(),
            client_ca_path: Some("ca.pem".into()),
        };
        assert!(tls.is_mtls());
    }
}
