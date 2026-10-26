FROM rust:slim as builder

WORKDIR /usr/src/

COPY . .

RUN apt-get update && apt-get install -y curl ca-certificates
RUN cargo build --release

FROM debian:trixie-slim

WORKDIR /usr/app

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/target/release/smtp2http smtp2http

CMD ["/usr/app/smtp2http"]
