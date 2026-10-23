use std::sync::Arc;

use bytes::BytesMut;
use postgres_protocol::message::frontend;
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::{CryptoProvider, ring, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime, pem::PemObject},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::TlsConnector;

use super::{BoxSocket, PgError};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// Encrypt without checking the certificate.
    None,
    /// The certificate must chain to a trusted CA, any host name.
    Ca,
    /// Trusted CA and a matching host name.
    Full,
}

#[derive(Clone)]
pub struct TlsSettings {
    connector: TlsConnector,
    server_name: ServerName<'static>,
}

pub enum Negotiated {
    Tls(BoxSocket),
    /// The server doesn't do TLS. The socket is still fine for a plain login.
    Declined(BoxSocket),
}

#[derive(Debug)]
struct AcceptAnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[derive(Debug)]
struct IgnoreHostName(Arc<WebPkiServerVerifier>);

impl ServerCertVerifier for IgnoreHostName {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // webpki checks the chain before the name, so a name error means the chain was trusted.
        match self.0.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now) {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

fn root_store(ca_file: Option<&str>) -> AppResult<RootCertStore> {
    let mut roots = RootCertStore::empty();

    let Some(path) = ca_file else {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        return Ok(roots);
    };

    let certificates = CertificateDer::pem_file_iter(path)
        .map_err(|error| AppError::general(format!("Couldn't read the CA certificate {path}: {error}")))?;
    for certificate in certificates {
        let certificate = certificate
            .map_err(|error| AppError::general(format!("Couldn't read the CA certificate {path}: {error}")))?;
        roots
            .add(certificate)
            .map_err(|error| AppError::general(format!("The CA certificate {path} isn't usable: {error}")))?;
    }

    if roots.is_empty() {
        return Err(AppError::general(format!("No certificates found in {path}")));
    }
    Ok(roots)
}

fn client_identity(cert_file: &str, key_file: &str) -> AppResult<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let certificates = CertificateDer::pem_file_iter(cert_file)
        .and_then(|certificates| certificates.collect::<Result<Vec<_>, _>>())
        .map_err(|error| AppError::general(format!("Couldn't read the client certificate {cert_file}: {error}")))?;
    if certificates.is_empty() {
        return Err(AppError::general(format!("No certificates found in {cert_file}")));
    }

    let key = PrivateKeyDer::from_pem_file(key_file)
        .map_err(|error| AppError::general(format!("Couldn't read the client key {key_file}: {error}")))?;
    Ok((certificates, key))
}

impl TlsSettings {
    /// `host` is the server's real name, which is what gets verified even when the socket goes through a tunnel.
    pub fn new(
        verification: Verification,
        host: &str,
        ca_file: Option<&str>,
        identity: Option<(&str, &str)>,
    ) -> AppResult<Self> {
        let provider = Arc::new(ring::default_provider());
        let tls_error = |error: rustls::Error| AppError::general(format!("TLS setup failed: {error}"));

        let verifier: Arc<dyn ServerCertVerifier> = match verification {
            Verification::None => Arc::new(AcceptAnyCertificate(provider.clone())),
            Verification::Ca | Verification::Full => {
                let webpki = WebPkiServerVerifier::builder_with_provider(Arc::new(root_store(ca_file)?), provider.clone())
                    .build()
                    .map_err(|error| AppError::general(format!("TLS setup failed: {error}")))?;
                if verification == Verification::Ca {
                    Arc::new(IgnoreHostName(webpki)) as Arc<dyn ServerCertVerifier>
                } else {
                    webpki as Arc<dyn ServerCertVerifier>
                }
            }
        };

        let builder = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(tls_error)?
            .dangerous()
            .with_custom_certificate_verifier(verifier);

        let config = match identity {
            Some((cert_file, key_file)) => {
                let (certificates, key) = client_identity(cert_file, key_file)?;
                builder.with_client_auth_cert(certificates, key).map_err(tls_error)?
            }
            None => builder.with_no_client_auth(),
        };

        let server_name = ServerName::try_from(host.trim().to_string())
            .map_err(|_| AppError::general(format!("{host} isn't a valid host name for TLS")))?;

        Ok(Self {
            connector: TlsConnector::from(Arc::new(config)),
            server_name,
        })
    }
}

pub async fn negotiate(mut socket: BoxSocket, settings: &TlsSettings) -> Result<Negotiated, PgError> {
    let mut request = BytesMut::new();
    frontend::ssl_request(&mut request);
    socket.write_all(&request).await?;
    socket.flush().await?;

    let mut answer = [0u8; 1];
    socket.read_exact(&mut answer).await?;

    match answer[0] {
        b'S' => {
            let stream = settings
                .connector
                .connect(settings.server_name.clone(), socket)
                .await
                .map_err(|error| PgError::Transport(format!("TLS handshake failed: {error}")))?;
            Ok(Negotiated::Tls(Box::new(stream)))
        }
        b'N' => Ok(Negotiated::Declined(socket)),
        other => Err(PgError::Transport(format!(
            "The server answered the TLS request with an unexpected byte ({other})"
        ))),
    }
}
