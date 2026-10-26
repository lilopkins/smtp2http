use std::time::Duration;

use color_eyre::{Result, eyre::eyre};
use reqwest::StatusCode;

use crate::settings::Settings;

const MAX_ATTEMPTS: usize = 6;
const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);

pub async fn send(settings: &Settings, client: &reqwest::Client, raw_email: &str) -> Result<()> {
    send_with_retry(
        settings,
        client,
        raw_email,
        MAX_ATTEMPTS,
        INITIAL_RETRY_DELAY,
    )
    .await
}

async fn send_with_retry(
    settings: &Settings,
    client: &reqwest::Client,
    raw_email: &str,
    max_attempts: usize,
    initial_retry_delay: Duration,
) -> Result<()> {
    assert!(max_attempts > 0, "max_attempts must be greater than zero");

    for attempt in 1..=max_attempts {
        match client
            .post(&settings.webhook)
            .header("Content-Type", "message/rfc822")
            .body(raw_email.to_owned())
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                tracing::info!("[Webhook] Successfully forwarded email.");
                return Ok(());
            }
            Ok(response) if is_retryable_status(response.status()) && attempt < max_attempts => {
                let delay = retry_delay(initial_retry_delay, attempt);
                tracing::warn!(
                    attempt,
                    status = %response.status(),
                    retry_after_seconds = delay.as_secs_f64(),
                    "[Webhook] Request failed; retrying"
                );
                tokio::time::sleep(delay).await;
            }
            Ok(response) => {
                return Err(eyre!(
                    "[Webhook] Endpoint returned HTTP status {} after {attempt} attempt(s)",
                    response.status()
                ));
            }
            Err(error) if attempt < max_attempts => {
                let delay = retry_delay(initial_retry_delay, attempt);
                tracing::warn!(
                    attempt,
                    error = %error,
                    retry_after_seconds = delay.as_secs_f64(),
                    "[Webhook] Request failed; retrying"
                );
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error.into()),
        }
    }

    unreachable!("a webhook attempt always returns")
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retry_delay(initial_delay: Duration, attempt: usize) -> Duration {
    initial_delay.saturating_mul(1_u32 << attempt.saturating_sub(1).min(31))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::{is_retryable_status, retry_delay, send_with_retry};
    use crate::settings::Settings;

    #[test]
    fn retries_only_transient_http_statuses() {
        assert!(is_retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(!is_retryable_status(reqwest::StatusCode::BAD_REQUEST));
    }

    #[test]
    fn backs_off_exponentially() {
        let initial_delay = std::time::Duration::from_secs(1);
        assert_eq!(retry_delay(initial_delay, 1), initial_delay);
        assert_eq!(
            retry_delay(initial_delay, 2),
            std::time::Duration::from_secs(2)
        );
        assert_eq!(
            retry_delay(initial_delay, 5),
            std::time::Duration::from_secs(16)
        );
    }

    #[tokio::test]
    async fn retries_transient_failures_until_the_webhook_succeeds() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            for status in [500, 503, 200] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 1024];
                assert!(socket.read(&mut request).await.unwrap() > 0);
                server_requests.fetch_add(1, Ordering::SeqCst);
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let settings = Settings {
            server_domain: "smtp.example.org".to_string(),
            smtp_port: 25,
            generate_tls_cert: false,
            disable_spf_validation: true,
            tls_cert: None,
            tls_key: None,
            webhook: format!("http://{address}"),
        };

        send_with_retry(
            &settings,
            &reqwest::Client::new(),
            "From: sender@example.org\r\n\r\nHello",
            3,
            std::time::Duration::ZERO,
        )
        .await
        .unwrap();
        server.await.unwrap();

        assert_eq!(requests.load(Ordering::SeqCst), 3);
    }
}
