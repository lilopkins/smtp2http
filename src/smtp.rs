use color_eyre::Result;
use reqwest::Client;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_rustls::TlsAcceptor;

use crate::{settings::Settings, webhook};

pub trait SmtpStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> SmtpStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub async fn handle_smtp_session(
    settings: &Settings,
    mut reader: BufReader<Box<dyn SmtpStream>>,
    tls_acceptor: TlsAcceptor,
    http_client: Client,
    is_tls: bool,
) -> Result<()> {
    send_line(
        &mut reader,
        &format!("220 {} ESMTP Ready\r\n", settings.server_domain),
    )
    .await?;

    let mut raw_email_data = String::new();
    let mut in_data_mode = false;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // Connection closed
        }

        if in_data_mode {
            if line == ".\r\n" || line == ".\n" {
                in_data_mode = false;
                send_line(&mut reader, "250 2.0.0 Ok: queued document\r\n").await?;

                webhook::send(settings, &http_client, &raw_email_data).await?;
                raw_email_data.clear();
            } else {
                let content = if line.starts_with("..") {
                    &line[1..]
                } else {
                    &line
                };
                raw_email_data.push_str(content);
            }
            continue;
        }

        let command_upper = line.trim().to_uppercase();

        if command_upper.starts_with("EHLO") || command_upper.starts_with("HELO") {
            let mut capabilities =
                format!("250-{} Hello\r\n250-8BITMIME\r\n", settings.server_domain);
            if !is_tls {
                capabilities.push_str("250-STARTTLS\r\n");
            }
            capabilities.push_str("250 OK\r\n");
            send_line(&mut reader, &capabilities).await?;
        } else if command_upper.starts_with("STARTTLS") && !is_tls {
            send_line(&mut reader, "220 2.0.0 Ready to start TLS\r\n").await?;

            let stream = reader.into_inner();
            let tls_stream = tls_acceptor.accept(stream).await?;

            // Re-wrap upgraded stream into worker session
            let stream: Box<dyn SmtpStream> = Box::new(tokio_rustls::TlsStream::Server(tls_stream));
            return Box::pin(handle_smtp_session(
                settings,
                BufReader::new(stream),
                tls_acceptor,
                http_client,
                true,
            ))
            .await;
        } else if command_upper.starts_with("MAIL FROM:") || command_upper.starts_with("RCPT TO:") {
            send_line(&mut reader, "250 2.1.0 Ok\r\n").await?;
        } else if command_upper == "DATA" {
            in_data_mode = true;
            send_line(&mut reader, "354 End data with <CR><LF>.<CR><LF>\r\n").await?;
        } else if command_upper == "QUIT" {
            send_line(&mut reader, "221 2.0.0 Bye\r\n").await?;
            break;
        } else if command_upper == "RSET" {
            raw_email_data.clear();
            send_line(&mut reader, "250 2.0.0 OK\r\n").await?;
        } else {
            send_line(&mut reader, "500 5.5.2 Command unrecognised\r\n").await?;
        }
    }

    Ok(())
}

async fn send_line<S: AsyncWriteExt + Unpin>(stream: &mut S, msg: &str) -> Result<()> {
    stream.write_all(msg.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}
