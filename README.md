# smtp2http

`smtp2http` accepts SMTP messages and forwards their RFC 822 source to an HTTP
webhook as a `POST` request with `Content-Type: message/rfc822`.
SPF is checked against the connecting client's IP and the `MAIL FROM` domain;
explicit SPF failures are rejected, and temporary DNS failures receive a
retryable SMTP response.

It can listen on standard SMTP with STARTTLS (port 25), SMTP (port 465), and
submission with STARTTLS (port 587).

## Configuration

Configuration is loaded from an optional `config` file in the working directory
(for example, `config.toml`) and then overridden by `SMTP2HTTP__` environment
variables.

| Environment variable | Config key | Default | Description |
| --- | --- | --- | --- |
| `SMTP2HTTP__SERVER_DOMAIN` | `server_domain` | `smtp2http.local` | Domain shown in SMTP greetings. |
| `SMTP2HTTP__SMTP_PORT` | `smtp_port` | `25` | SMTP listener port. |
| `SMTP2HTTP__TLS_CERT__FILE` | `tls_cert.file` | Required unless generating | TLS certificate PEM file. |
| `SMTP2HTTP__TLS_CERT__VALUE` | `tls_cert.value` | Required unless generating | TLS certificate PEM value. |
| `SMTP2HTTP__TLS_KEY__FILE` | `tls_key.file` | Required unless generating | TLS private key PEM file. |
| `SMTP2HTTP__TLS_KEY__VALUE` | `tls_key.value` | Required unless generating | TLS private key PEM value. |
| `SMTP2HTTP__GENERATE_TLS_CERT` | `generate_tls_cert` | `false` | Generate an ephemeral self-signed certificate for `server_domain` on startup instead of configuring a certificate and key. |
| `SMTP2HTTP__WEBHOOK` | `webhook` | Required | HTTP endpoint that receives each message. |
| `SMTP2HTTP__LOG` | N/A | Logging default | `tracing` filter, such as `info` or `debug`. |

## TLS certificate

For local development, generate a self-signed PEM certificate and private key:

```sh
mkdir -p certs
openssl req -x509 -newkey rsa:4096 -sha256 -nodes -days 365 \
  -keyout certs/tls.key \
  -out certs/tls.crt \
  -subj "/CN=localhost" \
  -addext "subjectAltName=DNS:localhost"
```

Configure `tls_cert.file` and `tls_key.file` (or their corresponding environment
variables) to use these files. To generate a new ephemeral self-signed
certificate on each startup instead, set `SMTP2HTTP__GENERATE_TLS_CERT=true` and
omit the certificate and key settings. This is useful for development; use a
certificate issued by a trusted certificate authority for production.

## Docker

Build the image, then mount the configuration and certificate directory. This
example maps the container's standard SMTP ports to unprivileged host ports:

```sh
docker build -t smtp2http .

docker run --rm \
  --name smtp2http \
  -p 2525:25 \
  -p 2465:465 \
  -p 2587:587 \
  -v "$PWD/config.toml:/usr/app/config.toml:ro" \
  -v "$PWD/certs:/certs:ro" \
  smtp2http
```

The paths in `config.toml` must refer to paths inside the container; with the
mount above, use `/certs/tls.crt` and `/certs/tls.key`.
