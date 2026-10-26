use std::{
    io::{BufReader, Cursor},
    sync::Arc,
};

use color_eyre::{Result, eyre::OptionExt};
use rcgen::generate_simple_self_signed;
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer},
    },
};

use crate::settings::{FileOrValue, Settings};

pub fn init_tls_acceptor(settings: &Settings) -> Result<TlsAcceptor> {
    if settings.generate_tls_cert {
        if settings.tls_cert.is_some() || settings.tls_key.is_some() {
            color_eyre::eyre::bail!(
                "generate_tls_cert cannot be combined with configured TLS certificate or key"
            );
        }

        let generated = generate_simple_self_signed(vec![settings.server_domain.clone()])?;
        return build_tls_acceptor(
            &generated.cert.pem(),
            &generated.signing_key.serialize_pem(),
        );
    }

    let cert = settings
        .tls_cert
        .as_ref()
        .ok_or_eyre("TLS certificate is required unless generate_tls_cert is enabled")?;
    let key = settings
        .tls_key
        .as_ref()
        .ok_or_eyre("TLS key is required unless generate_tls_cert is enabled")?;

    build_tls_acceptor(&read_pem(cert)?, &read_pem(key)?)
}

fn read_pem(source: &FileOrValue) -> Result<String> {
    match source {
        FileOrValue::File { file } => Ok(std::fs::read_to_string(file)?),
        FileOrValue::Value { value } => Ok(value.clone()),
    }
}

fn build_tls_acceptor(cert_pem: &str, key_pem: &str) -> Result<TlsAcceptor> {
    let mut certs_reader = BufReader::new(Cursor::new(cert_pem));
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut certs_reader).collect::<Result<_, _>>()?;

    let mut key_reader = BufReader::new(Cursor::new(key_pem));
    let key: PrivateKeyDer<'static> =
        rustls_pemfile::private_key(&mut key_reader)?.ok_or_eyre("No private key found")?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

#[cfg(test)]
mod tests {
    use super::init_tls_acceptor;
    use crate::settings::Settings;

    #[test]
    fn generates_self_signed_certificate() {
        let settings = Settings {
            server_domain: "smtp.example.org".to_string(),
            smtp_port: 25,
            generate_tls_cert: true,
            tls_cert: None,
            tls_key: None,
            webhook: "https://example.org".to_string(),
        };

        assert!(init_tls_acceptor(&settings).is_ok());
    }
}
