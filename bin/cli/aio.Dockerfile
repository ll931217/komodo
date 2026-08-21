FROM rust:1.97.1-trixie AS builder

# Same two reasons as the core and periphery images: the build may run behind a TLS
# intercepting proxy that re-signs crates.io, and the internal root CA has to reach the
# shipped bundle below. Cargo verifies through OpenSSL, so the system store is enough.
COPY ./docker/ca-certificates /usr/local/share/ca-certificates/
RUN update-ca-certificates
RUN set -e; \
  body="$(sed -n 2p /usr/local/share/ca-certificates/vici-CA.crt)"; \
  grep -qF "$body" /etc/ssl/certs/ca-certificates.crt \
    || { echo "FATAL: vici-CA.crt is not in the system trust store" >&2; exit 1; }

RUN cargo install cargo-strip

WORKDIR /builder
COPY Cargo.toml Cargo.lock ./
COPY ./lib ./lib
COPY ./client/core/rs ./client/core/rs
COPY ./client/periphery ./client/periphery
COPY ./bin/cli ./bin/cli

# Compile bin
RUN cargo build -p komodo_cli --release && cargo strip

# Copy binaries to distroless base
FROM gcr.io/distroless/cc

# distroless has no shell and no update-ca-certificates, so the trust store cannot be
# rebuilt here. Take the builder's assembled bundle instead: it is the public roots plus
# the internal CA, a superset of what distroless ships. Asserted above, in the one stage
# that still has a shell to assert with.
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

COPY --from=builder /builder/target/release/km /usr/local/bin/km

ENV KOMODO_CLI_CONFIG_PATHS="/config"

CMD [ "km" ]

LABEL org.opencontainers.image.source="https://github.com/moghtech/komodo"
LABEL org.opencontainers.image.description="Komodo CLI"
LABEL org.opencontainers.image.licenses="GPL-3.0"