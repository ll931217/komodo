## All in one, multi stage compile + runtime Docker build for your architecture.

# Cluster support shells out to kubectl, which has to be in the image.
# Both k8s binary hosts are blocked at the corporate proxy (pkgs.k8s.io and
# dl.k8s.io never open a connection), while Docker Hub passes — so kubectl is
# lifted out of kindest/node, which ships /usr/bin/kubectl built from the
# matching upstream tag. Same approach as
# terraform/live/local/kubeadm/kubeadm-common.sh in aws-staging.
# Keep K8S_VERSION in step with the clusters this Periphery talks to.
ARG K8S_VERSION=v1.33.12

# Helm release support shells out to helm. get.helm.sh is blocked the
# same way the k8s hosts are, so the static binary is lifted out of the
# alpine/helm image on Docker Hub (Go binary, runs fine on debian).
#
# Both ARGs belong here, above the first FROM: an ARG declared after one
# is scoped to that stage, so HELM_VERSION would expand to empty in the
# FROM below ("invalid reference format"). Recent BuildKit papers over
# it; the builder in docker 24 does not.
ARG HELM_VERSION=3.19.0

# Terraform units shell out to terraform. releases.hashicorp.com is blocked
# like the k8s hosts, so the binary is lifted from the Docker Hub image.
# Pin matches aws-staging Makefile:42 so both run the same terraform.
ARG TERRAFORM_VERSION=1.15.8

FROM docker.io/kindest/node:${K8S_VERSION} AS kubectl

FROM docker.io/alpine/helm:${HELM_VERSION} AS helm

FROM docker.io/hashicorp/terraform:${TERRAFORM_VERSION} AS terraform

# registry.terraform.io is blocked too, so providers cannot be resolved at
# apply time — they are baked in as a filesystem mirror instead. Only the two
# the Kubernetes units need: 37MB combined, against 179MB for aws alone, which
# nothing here runs. A unit needing other providers can still mount its own
# mirror; provider_installation takes both.
#
# Source is OpenTofu's re-releases of the same providers on github.com, which
# passes the proxy — the same source aws-staging's CI uses.
FROM debian:trixie-slim AS providers
ARG K8S_PROVIDER=2.38.0
ARG HELM_PROVIDER=3.2.0
ARG HTTPS_PROXY
ARG HTTP_PROXY
ARG NO_PROXY
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates unzip \
  && rm -rf /var/lib/apt/lists/*
COPY ./docker/ca-certificates /usr/local/share/ca-certificates/
RUN update-ca-certificates
RUN set -eux; \
  base=/mirror/registry.terraform.io/hashicorp; \
  for spec in "kubernetes=${K8S_PROVIDER}" "helm=${HELM_PROVIDER}"; do \
    name="${spec%%=*}"; version="${spec#*=}"; \
    zip="terraform-provider-${name}_${version}_linux_amd64.zip"; \
    mkdir -p "${base}/${name}"; \
    curl -fsSL -o "${base}/${name}/${zip}" \
      "https://github.com/opentofu/terraform-provider-${name}/releases/download/v${version}/${zip}"; \
    unzip -tqq "${base}/${name}/${zip}"; \
  done

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

# Fail the build if the internal root CA did not reach the trust store. Without this
# the image builds green and the failure surfaces much later, as a TLS handshake error
# against Core / harbor / the doc host on whichever machine happens to run it. Matches
# a body line of the cert against the assembled bundle, so it needs no openssl.
RUN set -e; \
  body="$(sed -n 2p /usr/local/share/ca-certificates/vici-CA.crt)"; \
  grep -qF "$body" /etc/ssl/certs/ca-certificates.crt \
    || { echo "FATAL: vici-CA.crt is not in the system trust store" >&2; exit 1; }

COPY --from=builder /builder/target/release/periphery /usr/local/bin/periphery
COPY --from=kubectl /usr/bin/kubectl /usr/local/bin/kubectl
COPY --from=helm /usr/bin/helm /usr/local/bin/helm
COPY --from=terraform /bin/terraform /usr/local/bin/terraform
COPY --from=providers /mirror /usr/local/share/terraform/mirror

# `direct` is excluded rather than omitted: without it terraform falls back to
# registry.terraform.io for anything missing from the mirror and hangs on the
# blocked host instead of failing with a clear "no available releases".
RUN printf '%s\n' \
  'provider_installation {' \
  '  filesystem_mirror {' \
  '    path    = "/usr/local/share/terraform/mirror"' \
  '    include = ["registry.terraform.io/*/*"]' \
  '  }' \
  '  direct { exclude = ["registry.terraform.io/*/*"] }' \
  '}' > /usr/local/share/terraform/terraformrc
ENV TF_CLI_CONFIG_FILE=/usr/local/share/terraform/terraformrc

# Assert the lift landed a runnable binary rather than trusting the COPY: a
# wrong path in kindest/node would otherwise only surface at cluster-op time.
RUN kubectl version --client=true -o yaml | grep -q gitVersion
RUN helm version --short | grep -q v3
RUN terraform version | grep -q Terraform
# And that the mirror is actually populated — an empty COPY would otherwise
# only surface as a failed init on the first real unit. Counted rather than
# named, so a version bump does not need this line edited too.
RUN test "$(find /usr/local/share/terraform/mirror -name '*.zip' -size +1M | wc -l)" -eq 2

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
