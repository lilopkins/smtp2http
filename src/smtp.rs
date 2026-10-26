use color_eyre::Result;
use mail_auth::{MessageAuthenticator, SpfResult, spf::verify::SpfParameters};
use reqwest::Client;
use std::net::IpAddr;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_rustls::TlsAcceptor;

use crate::{settings::Settings, webhook};

pub trait SmtpStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> SmtpStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub async fn handle_smtp_session(
    settings: &Settings,
    mut reader: BufReader<Box<dyn SmtpStream>>,
    client_ip: IpAddr,
    tls_acceptor: TlsAcceptor,
    http_client: Client,
    spf_authenticator: MessageAuthenticator,
    is_tls: bool,
) -> Result<()> {
    send_line(
        &mut reader,
        &format!("220 {} ESMTP Ready\r\n", settings.server_domain),
    )
    .await?;

    let mut raw_email_data = String::new();
    let mut in_data_mode = false;
    let mut helo_domain = String::new();
    let mut mail_from_accepted = false;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // Connection closed
        }

        tracing::debug!("[SMTP] -> {line}");

        if in_data_mode {
            if line == ".\r\n" || line == ".\n" {
                in_data_mode = false;
                send_line(&mut reader, "250 2.0.0 Ok: queued document\r\n").await?;

                webhook::send(settings, &http_client, &raw_email_data).await?;
                raw_email_data.clear();
                mail_from_accepted = false;
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
            helo_domain = line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();
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
                client_ip,
                tls_acceptor,
                http_client,
                spf_authenticator,
                true,
            ))
            .await;
        } else if command_upper.starts_with("MAIL FROM:") {
            if let Some(sender) = parse_mail_from(&line) {
                let (accepted, response) = validate_sender(
                    settings,
                    &spf_authenticator,
                    client_ip,
                    &helo_domain,
                    sender,
                )
                .await;
                mail_from_accepted = accepted;
                send_line(&mut reader, response).await?;
            } else {
                mail_from_accepted = false;
                send_line(&mut reader, "501 5.5.2 Invalid MAIL FROM\r\n").await?;
            }
        } else if command_upper.starts_with("RCPT TO:") {
            send_line(&mut reader, "250 2.1.0 Ok\r\n").await?;
        } else if command_upper == "DATA" {
            if mail_from_accepted {
                in_data_mode = true;
                send_line(&mut reader, "354 End data with <CR><LF>.<CR><LF>\r\n").await?;
            } else {
                send_line(&mut reader, "503 5.5.1 MAIL FROM required\r\n").await?;
            }
        } else if command_upper == "QUIT" {
            send_line(&mut reader, "221 2.0.0 Bye\r\n").await?;
            break;
        } else if command_upper == "RSET" {
            raw_email_data.clear();
            mail_from_accepted = false;
            send_line(&mut reader, "250 2.0.0 OK\r\n").await?;
        } else {
            send_line(&mut reader, "500 5.5.2 Command unrecognised\r\n").await?;
        }
    }

    Ok(())
}

fn parse_mail_from(command: &str) -> Option<&str> {
    let command = command.trim();
    let (verb, path) = command.split_once(':')?;
    if !verb.eq_ignore_ascii_case("MAIL FROM") {
        return None;
    }

    let path = path.trim_start().strip_prefix('<')?;
    let (sender, parameters) = path.split_once('>')?;
    if !parameters.is_empty() && !parameters.starts_with(char::is_whitespace) {
        return None;
    }
    Some(sender)
}

async fn verify_spf(
    authenticator: &MessageAuthenticator,
    client_ip: IpAddr,
    helo_domain: &str,
    host_domain: &str,
    sender: &str,
) -> SpfResult {
    authenticator
        .verify_spf(SpfParameters::mail_from(
            client_ip,
            helo_domain,
            host_domain,
            sender,
        ))
        .await
        .result()
}

async fn validate_sender(
    settings: &Settings,
    authenticator: &MessageAuthenticator,
    client_ip: IpAddr,
    helo_domain: &str,
    sender: &str,
) -> (bool, &'static str) {
    if settings.disable_spf_validation {
        return (true, "250 2.1.0 Ok\r\n");
    }

    let result = verify_spf(
        authenticator,
        client_ip,
        helo_domain,
        &settings.server_domain,
        sender,
    )
    .await;
    (
        !matches!(result, SpfResult::Fail | SpfResult::TempError),
        spf_response(result),
    )
}

const fn spf_response(result: SpfResult) -> &'static str {
    match result {
        SpfResult::Fail => "550 5.7.23 SPF validation failed\r\n",
        SpfResult::TempError => "451 4.4.3 Temporary SPF lookup failure\r\n",
        _ => "250 2.1.0 Ok\r\n",
    }
}

async fn send_line<S: AsyncWriteExt + Unpin>(stream: &mut S, msg: &str) -> Result<()> {
    tracing::debug!("[SMTP] <- {msg}");
    stream.write_all(msg.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_mail_from, spf_response};
    use mail_auth::SpfResult;

    #[test]
    fn parses_mail_from_path() {
        assert_eq!(
            parse_mail_from("MAIL FROM:<sender@example.org>\r\n"),
            Some("sender@example.org")
        );
        assert_eq!(parse_mail_from("mail from: <> SIZE=10\r\n"), Some(""));
        assert_eq!(parse_mail_from("MAIL FROM: sender@example.org\r\n"), None);
    }

    #[test]
    fn maps_spf_results_to_smtp_responses() {
        assert!(spf_response(SpfResult::Fail).starts_with("550 "));
        assert!(spf_response(SpfResult::TempError).starts_with("451 "));
        assert!(spf_response(SpfResult::Pass).starts_with("250 "));
        assert!(spf_response(SpfResult::SoftFail).starts_with("250 "));
        assert!(spf_response(SpfResult::Neutral).starts_with("250 "));
        assert!(spf_response(SpfResult::None).starts_with("250 "));
        assert!(spf_response(SpfResult::PermError).starts_with("250 "));
    }
}
