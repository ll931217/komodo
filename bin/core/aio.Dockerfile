## All in one, multi stage compile + runtime Docker build for your architecture.

# Build Core
FROM rust:1.97.1-trixie AS core-builder

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
COPY ./bin/core ./bin/core
COPY ./bin/cli ./bin/cli
COPY ./xtask ./xtask
# Workspace member: cargo cannot load the workspace without its manifest,
# even though nothing here builds it.
COPY ./e2e ./e2e

# Compile app
RUN cargo build -p komodo_core --release && \
  cargo build -p komodo_cli --release && \
  cargo strip

# Build UI
FROM node:22.12-alpine AS ui-builder

# Node ships its own roots and ignores the system store, so the extra
# certificates are handed to it directly. The bundle is empty by
# default, which node accepts silently.
COPY ./docker/ca-certificates /ca-certificates/
# Each cert gets a newline after it: PEM files do not reliably end with
# one, and gluing an END line to the next BEGIN makes node reject the
# whole bundle ("bad end line") and silently fall back to public roots.
RUN set -e; : > /extra-ca.pem; \
  for cert in /ca-certificates/*.crt; do \
    [ -f "$cert" ] || continue; \
    cat "$cert" >> /extra-ca.pem; \
    echo >> /extra-ca.pem; \
  done
ENV NODE_EXTRA_CA_CERTS=/extra-ca.pem

# Defaults to yarn's own registry, so this changes nothing normally.
# Override when a proxy will not serve registry.yarnpkg.com:
#   --build-arg YARN_REGISTRY=https://registry.npmjs.org/
ARG YARN_REGISTRY=https://registry.yarnpkg.com
ENV YARN_REGISTRY=${YARN_REGISTRY}

WORKDIR /builder
COPY ./ui ./ui
COPY ./client/core/ts ./client

# yarn.lock pins absolute tarball urls, so overriding the registry alone
# does not redirect the downloads. Rewrites only when the registry was
# actually overridden, so upstream builds are untouched. Tarballs are
# identical across registries and the lockfile's integrity hashes still
# verify them.
RUN if [ "${YARN_REGISTRY%/}" != "https://registry.yarnpkg.com" ]; then \
      find . -name yarn.lock -exec \
        sed -i "s|https://registry.yarnpkg.com/|${YARN_REGISTRY%/}/|g" {} +; \
    fi

RUN cd client && yarn && yarn build && yarn link
RUN cd ui && yarn link komodo_client && yarn && yarn build

# Final Image
FROM debian:trixie-slim

COPY ./bin/core/starship.toml /starship.toml

# Placed before the deps script so the ca-certificates package it
# installs picks these up, which the script's own curl to starship.rs
# and the deno install below then rely on.
COPY ./docker/ca-certificates /usr/local/share/ca-certificates/

COPY ./bin/core/debian-deps.sh .
RUN sh ./debian-deps.sh && rm ./debian-deps.sh

# Fail the build if the internal root CA did not reach the trust store. Without this
# the image builds green and the failure surfaces much later, as a TLS handshake error
# against Core / harbor / the doc host on whichever machine happens to run it. Matches
# a body line of the cert against the assembled bundle, so it needs no openssl.
RUN set -e; \
  body="$(sed -n 2p /usr/local/share/ca-certificates/vici-CA.crt)"; \
  grep -qF "$body" /etc/ssl/certs/ca-certificates.crt \
    || { echo "FATAL: vici-CA.crt is not in the system trust store" >&2; exit 1; }

# Deno bundles its own roots; point it at the system store so it sees
# any extra certificates too. Harmless when there are none.
ENV DENO_CERT=/etc/ssl/certs/ca-certificates.crt

# Setup an application directory
WORKDIR /app

# Copy
COPY ./config/core.config.toml /config/.default.config.toml
COPY --from=ui-builder /builder/ui/dist /app/ui
COPY --from=core-builder /builder/target/release/core /usr/local/bin/core
COPY --from=core-builder /builder/target/release/km /usr/local/bin/km
COPY --from=denoland/deno:bin /deno /usr/local/bin/deno

# Set $DENO_DIR and preload external Deno deps.
# FORK: npm packages rather than jsr:@std/*, which this network blocks.
# Must match the specifiers injected in bin/core/src/api/execute/action.rs
# or Actions re-resolve them at run time.
ENV DENO_DIR=/action-cache/deno
RUN mkdir /action-cache && \
  cd /action-cache && \
  deno install npm:js-yaml@4.1.0 npm:smol-toml@1.4.2

COPY ./bin/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Hint at the port
EXPOSE 9120

ENV KOMODO_CLI_CONFIG_PATHS="/config"
# This ensures any `komodo.cli.*` takes precedence over the Core `/config/*config.*`
ENV KOMODO_CLI_CONFIG_KEYWORDS="*config.*,*komodo.cli*.*"

ENTRYPOINT [ "entrypoint.sh" ]
CMD [ "core" ]

# Label to prevent Komodo from stopping with StopAllContainers
LABEL komodo.skip="true"
# Label for Ghcr
LABEL org.opencontainers.image.source="https://github.com/moghtech/komodo"
LABEL org.opencontainers.image.description="Komodo Core"
LABEL org.opencontainers.image.licenses="GPL-3.0"
