#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(rust_2018_idioms)]

use color_eyre::Result;
use config::Config;
use mail_auth::MessageAuthenticator;
use reqwest::{
    Client,
    header::{self, HeaderMap, HeaderValue},
};
use tokio::{io::BufReader, net::TcpListener};
use tokio_rustls::{TlsAcceptor, TlsStream};
use tracing_subscriber::EnvFilter;

mod settings;
mod smtp;
mod socket;
mod tls;
mod webhook;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_env("SMTP2HTTP_LOG"))
        .init();
    let settings: settings::Settings = Config::builder()
        .add_source(config::File::with_name("config").required(false))
        .add_source(config::Environment::with_prefix("SMTP2HTTP").separator("__"))
        .build()?
        .try_deserialize()?;
    tracing::debug!("{settings:#?}");

    let tls_acceptor = tls::init_tls_acceptor(&settings)?;
    let spf_authenticator = MessageAuthenticator::new_system_conf()?;
    let http_client = reqwest::Client::builder()
        .default_headers({
            let mut map = HeaderMap::new();
            map.insert(
                header::USER_AGENT,
                HeaderValue::from_bytes(
                    format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).as_bytes(),
                )?,
            );
            map
        })
        .build()?;

    let prefix = "SMTP";
    let listener = socket::bind(settings.smtp_port)?;
    handle_connections(
        settings.clone(),
        prefix,
        listener,
        false,
        tls_acceptor.clone(),
        http_client.clone(),
        spf_authenticator.clone(),
    )
    .await;

    Ok(())
}

async fn handle_connections(
    settings: settings::Settings,
    prefix: &str,
    listener: TcpListener,
    require_tls_from_start: bool,
    tls_acceptor: TlsAcceptor,
    http_client: Client,
    spf_authenticator: MessageAuthenticator,
) {
    loop {
        match listener.accept().await {
            Ok((socket, peer_addr)) => {
                tracing::info!("[{prefix}] Incoming from: {peer_addr}");
                let prefix = prefix.to_string();
                let tls_acceptor = tls_acceptor.clone();
                let settings = settings.clone();
                let http_client = http_client.clone();
                let spf_authenticator = spf_authenticator.clone();
                tokio::spawn(async move {
                    if require_tls_from_start {
                        match tls_acceptor.accept(socket).await {
                            Ok(tls_stream) => {
                                let stream: Box<dyn smtp::SmtpStream> =
                                    Box::new(TlsStream::Server(tls_stream));
                                let reader = BufReader::new(stream);
                                if let Err(e) = smtp::handle_smtp_session(
                                    &settings,
                                    reader,
                                    peer_addr.ip(),
                                    tls_acceptor,
                                    http_client,
                                    spf_authenticator,
                                    true,
                                )
                                .await
                                {
                                    eprintln!("[{prefix}] SMTP session failed: {e:?}");
                                }
                            }
                            Err(e) => eprintln!("[{prefix}] TLS handshake failed: {e:?}"),
                        }
                    } else {
                        let stream: Box<dyn smtp::SmtpStream> = Box::new(socket);
                        let reader = BufReader::new(stream);
                        if let Err(e) = smtp::handle_smtp_session(
                            &settings,
                            reader,
                            peer_addr.ip(),
                            tls_acceptor,
                            http_client,
                            spf_authenticator,
                            false,
                        )
                        .await
                        {
                            eprintln!("[{prefix}] SMTP session failed: {e:?}");
                        }
                    }
                });
            }
            Err(e) => eprintln!("[{prefix}] Accept error: {e:?}"),
        }
    }
}
