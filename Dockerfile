FROM rust:slim as builder
RUN apt-get update && apt-get install -y ca-certificates

WORKDIR /usr/src/
COPY . .
RUN cargo build --release

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/app
COPY --from=builder /usr/src/target/release/smtp2http smtp2http

CMD ["/usr/app/smtp2http"]
