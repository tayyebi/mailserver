use anyhow::Result;
use rcgen::generate_simple_self_signed;
use rustls::{Certificate, PrivateKey, ServerConfig};
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

pub fn create_tls_acceptor() -> Result<TlsAcceptor> {
    // Generate a self-signed certificate
    let subject_alt_names = vec!["localhost".to_string(), "example.com".to_string()];
    let cert = generate_simple_self_signed(subject_alt_names)?;
    
    let cert_der = cert.serialize_der()?;
    let key_der = cert.serialize_private_key_der();
    
    let cert_chain = vec![Certificate(cert_der)];
    let key = PrivateKey(key_der);

    let config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}
