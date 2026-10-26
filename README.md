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
(for example, `config.toml`) and then overridden by `SMTP2HTTP_` environment
variables.

| Environment variable | Config key | Default | Description |
| --- | --- | --- | --- |
| `SMTP2HTTP_SERVER_DOMAIN` | `server_domain` | `smtp2http.local` | Domain shown in SMTP greetings. |
| `SMTP2HTTP_SMTP_PORT` | `smtp_port` | `25` | SMTP listener port. |
| `SMTP2HTTP_TLS_CERT__PATH` | `tls_cert.path` | Required | TLS certificate path. Mutually exclusive with TLS certificate value. |
| `SMTP2HTTP_TLS_CERT__VALUE` | `tls_cert.value` | Required | TLS certificate value. Mutually exclusive with TLS certificate path. |
| `SMTP2HTTP_TLS_KEY__PATH` | `tls_key.path` | Required | TLS key path. Mutually exclusive with TLS key value. |
| `SMTP2HTTP_TLS_KEY__VALUE` | `tls_key.value` | Required | TLS key value. Mutually exclusive with TLS key path. |
| `SMTP2HTTP_WEBHOOK` | `webhook` | Required | HTTP endpoint that receives each message. |
| `SMTP2HTTP_LOG` | N/A | Logging default | `tracing` filter, such as `info` or `debug`. |

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

Configure `tls_cert.path` and `tls_key.path` (or their corresponding environment
variables) to use these files. Use a certificate issued by a trusted certificate
authority for production.

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
