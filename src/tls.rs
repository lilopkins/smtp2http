use std::{
    fs::File,
    io::{BufReader, Cursor},
    sync::Arc,
};

use color_eyre::{Result, eyre::OptionExt};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer},
    },
};

use crate::settings::{FileOrValue, Settings};

pub fn init_tls_acceptor(settings: &Settings) -> Result<TlsAcceptor> {
    let certs: Vec<CertificateDer<'static>> = match &settings.tls_cert {
        FileOrValue::File { path } => {
            let mut certs_reader = BufReader::new(File::open(path)?);
            rustls_pemfile::certs(&mut certs_reader).collect::<Result<_, _>>()?
        }
        FileOrValue::Value { value } => {
            let mut certs_reader = BufReader::new(Cursor::new(value));
            rustls_pemfile::certs(&mut certs_reader).collect::<Result<_, _>>()?
        }
    };

    let key: PrivateKeyDer<'static> = match &settings.tls_key {
        FileOrValue::File { path } => {
            let mut key_reader = BufReader::new(File::open(path)?);
            rustls_pemfile::private_key(&mut key_reader)?.ok_or_eyre("No private key found")?
        }
        FileOrValue::Value { value } => {
            let mut key_reader = BufReader::new(Cursor::new(value));
            rustls_pemfile::private_key(&mut key_reader)?.ok_or_eyre("No private key found")?
        }
    };

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
