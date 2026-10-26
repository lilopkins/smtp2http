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
use std::future::Future;
use tokio::{io::BufReader, net::TcpListener, task::JoinSet};
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

    let listener = socket::bind(settings.smtp_port)?;
    let shutdown = shutdown_signal()?;
    handle_connections(
        listener,
        ConnectionHandler {
            settings: settings.clone(),
            prefix: "SMTP".to_string(),
            require_tls_from_start: false,
            tls_acceptor,
            http_client,
            spf_authenticator,
        },
        shutdown,
    )
    .await?;

    Ok(())
}

#[cfg(unix)]
fn shutdown_signal() -> Result<impl Future<Output = Result<()>>> {
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    Ok(async move {
        tokio::select! {
            _ = interrupt.recv() => tracing::info!("Received interrupt signal; shutting down"),
            _ = terminate.recv() => tracing::info!("Received SIGTERM; shutting down"),
        }

        Ok(())
    })
}

#[cfg(not(unix))]
fn shutdown_signal() -> Result<impl Future<Output = Result<()>>> {
    Ok(async {
        tokio::signal::ctrl_c().await?;
        tracing::info!("Received interrupt signal; shutting down");
        Ok(())
    })
}

#[derive(Clone)]
struct ConnectionHandler {
    settings: settings::Settings,
    prefix: String,
    require_tls_from_start: bool,
    tls_acceptor: TlsAcceptor,
    http_client: Client,
    spf_authenticator: MessageAuthenticator,
}

async fn handle_connections<F>(
    listener: TcpListener,
    handler: ConnectionHandler,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = Result<()>>,
{
    tokio::pin!(shutdown);
    let mut sessions = JoinSet::new();

    loop {
        tokio::select! {
            signal = &mut shutdown => {
                signal?;
                break;
            }
            Some(result) = sessions.join_next(), if !sessions.is_empty() => {
                if let Err(error) = result
                    && !error.is_cancelled() {
                        tracing::error!("SMTP session task failed: {error}");
                }
            }
            accepted = listener.accept() => match accepted {
                Ok((socket, peer_addr)) => {
                tracing::info!("[{}] Incoming from: {peer_addr}", handler.prefix);
                let handler = handler.clone();
                sessions.spawn(async move {
                    if handler.require_tls_from_start {
                        match handler.tls_acceptor.accept(socket).await {
                            Ok(tls_stream) => {
                                let stream: Box<dyn smtp::SmtpStream> =
                                    Box::new(TlsStream::Server(tls_stream));
                                let reader = BufReader::new(stream);
                                if let Err(e) = smtp::handle_smtp_session(
                                    &handler.settings,
                                    reader,
                                    peer_addr.ip(),
                                    handler.tls_acceptor,
                                    handler.http_client,
                                    handler.spf_authenticator,
                                    true,
                                )
                                .await
                                {
                                    eprintln!("[{}] SMTP session failed: {e:?}", handler.prefix);
                                }
                            }
                            Err(e) => eprintln!("[{}] TLS handshake failed: {e:?}", handler.prefix),
                        }
                    } else {
                        let stream: Box<dyn smtp::SmtpStream> = Box::new(socket);
                        let reader = BufReader::new(stream);
                        if let Err(e) = smtp::handle_smtp_session(
                            &handler.settings,
                            reader,
                            peer_addr.ip(),
                            handler.tls_acceptor,
                            handler.http_client,
                            handler.spf_authenticator,
                            false,
                        )
                        .await
                        {
                            eprintln!("[{}] SMTP session failed: {e:?}", handler.prefix);
                        }
                    }
                });
                }
                Err(error) => tracing::error!("[{}] Accept error: {error:?}", handler.prefix),
            },
        }
    }

    tracing::info!("Stopping {} active SMTP session(s)", sessions.len());
    sessions.abort_all();
    while let Some(result) = sessions.join_next().await {
        if let Err(error) = result
            && !error.is_cancelled()
        {
            tracing::error!("SMTP session task failed during shutdown: {error}");
        }
    }

    Ok(())
}
