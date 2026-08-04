## All in one, multi stage compile + runtime Docker build for your architecture.

FROM rust:1.97.1-trixie AS builder

# Extra CA certificates, for networks that intercept TLS. Ships empty,
# so this is a no-op unless certs are dropped in. See the directory's
# README. Cargo verifies through OpenSSL, so the system store is enough.
COPY ./docker/ca-certificates /usr/local/share/ca-certificates/
RUN update-ca-certificates

RUN cargo install cargo-strip

WORKDIR /builder
COPY Cargo.toml Cargo.lock ./
COPY ./lib ./lib
COPY ./client/core/rs ./client/core/rs
COPY ./client/periphery ./client/periphery
COPY ./bin/periphery ./bin/periphery
COPY ./xtask ./xtask
# Workspace member: cargo cannot load the workspace without its manifest,
# even though nothing here builds it.
COPY ./e2e ./e2e

# Compile app
RUN cargo build -p komodo_periphery --release && cargo strip

# Final Image
FROM debian:trixie-slim

COPY ./bin/periphery/starship.toml /starship.toml

# Placed before the deps script so the ca-certificates package it
# installs picks these up, which the script's own curl then relies on.
COPY ./docker/ca-certificates /usr/local/share/ca-certificates/

COPY ./bin/periphery/debian-deps.sh .
RUN sh ./debian-deps.sh && rm ./debian-deps.sh

COPY --from=builder /builder/target/release/periphery /usr/local/bin/periphery

COPY ./bin/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 8120

# Can mount config file to /config/*config*.toml and it will be picked up.
ENV PERIPHERY_CONFIG_PATHS="/config"
# Change the default in container to /config/keys to match Core
ENV PERIPHERY_PRIVATE_KEY="file:/config/keys/periphery.key"

ENTRYPOINT [ "entrypoint.sh" ]
CMD [ "periphery" ]

# Label to prevent Komodo from stopping with StopAllContainers
LABEL komodo.skip="true"
# Label for ghcr
LABEL org.opencontainers.image.source="https://github.com/moghtech/komodo"
LABEL org.opencontainers.image.description="Komodo Periphery"
LABEL org.opencontainers.image.licenses="GPL-3.0"
