# Komodo task runner.
#
# Mirrors the recipes in runfile.toml so they work without
# runnables-cli, and bakes in the environment quirks that otherwise
# cost an afternoon (yarn registry, the komodo_client symlink).
#
# `make` lists every target. Every target takes ARGS for extra flags:
#   make check ARGS="-p komodo_core"
# and anything not covered has an escape hatch:
#   make cmd C='cargo tree -d'
# In C, double any shell '$' so make does not eat it first:
#   make cmd C='echo $$PWD'

SHELL := /bin/bash
.DEFAULT_GOAL := help

# yarn defaults to registry.yarnpkg.com, which the corporate proxy
# answers with 403. registry.npmjs.org is reachable.
REGISTRY ?= https://registry.npmjs.org/
CORE_CONFIG ?= .dev/core.config.toml
PERIPHERY_CONFIG ?= .dev/periphery.config.toml
ARGS ?=

.PHONY: help
help: ## List targets
	@echo "Komodo targets (make <target>):"
	@grep -hE '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) \
	  | awk -F':.*?## ' '{printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'

.PHONY: cmd
cmd: ## Run any command in the repo root: make cmd C='cargo tree -d'
	@if [ -z "$(C)" ]; then echo "usage: make cmd C='<command>'" >&2; exit 2; fi
	$(C)

## --- setup ---

.PHONY: deps
deps: ## Install js dependencies (ui, docsite, ts client)
	cd ui && yarn --registry $(REGISTRY)
	cd docsite && yarn --registry $(REGISTRY)
	cd client/core/ts && yarn --registry $(REGISTRY)

.PHONY: link-client
link-client: ## Re-link komodo_client into ui (any yarn install drops this symlink)
	cd client/core/ts && yarn link
	cd ui && yarn link komodo_client

.PHONY: dev-config
dev-config: ## Seed .dev/ configs from the templates in config/
	mkdir -p .dev
	@test -f $(CORE_CONFIG) || cp config/core.config.toml $(CORE_CONFIG)
	@test -f $(PERIPHERY_CONFIG) || cp config/periphery.config.toml $(PERIPHERY_CONFIG)
	@echo "configs at .dev/ - set the database address before 'make core'"

## --- codegen ---

.PHONY: gen-client
gen-client: ## Regenerate the ts client from the Rust types and copy into ui
	node ./client/core/ts/generate_types.mjs
	cd client/core/ts && yarn build
	cp -r client/core/ts/dist/. ui/public/client/.
	cd ui && node fix_public_client.mjs

.PHONY: gen-schema
gen-schema: ## Regenerate the resource toml json schema
	cargo xtask generate resource-schema --pretty --file=./ui/public/schema/resources.json

.PHONY: gen
gen: gen-client gen-schema ## Run all codegen

## --- dev servers ---

.PHONY: core
core: ## Run Core against .dev/core.config.toml
	KOMODO_CONFIG_PATH=$(CORE_CONFIG) cargo run -p komodo_core --release $(ARGS)

.PHONY: periphery
periphery: ## Run Periphery against .dev/periphery.config.toml
	cargo run -p komodo_periphery --release -- -c $(PERIPHERY_CONFIG) $(ARGS)

.PHONY: ui
ui: ## Run the ui dev server (set VITE_KOMODO_HOST in ui/.env.development)
	cd ui && yarn dev $(ARGS)

.PHONY: docs
docs: ## Serve the rustdoc site on :8050
	cargo doc --no-deps -p komodo_client && http-server -p 8050 target/doc

## --- verify ---

.PHONY: check
check: ## cargo check the workspace including tests
	cargo check --workspace --tests $(ARGS)

.PHONY: test
test: ## Unit tests (excludes e2e, which needs the live stack)
	cargo test --workspace --exclude komodo_e2e $(ARGS)

.PHONY: fmt
fmt: ## Format rust
	cargo fmt $(ARGS)

.PHONY: lint
lint: ## cargo fmt --check + clippy + ui tsc
	cargo fmt --check
	cargo clippy --workspace --tests $(ARGS)
	cd ui && npx tsc --noEmit

.PHONY: tsc
tsc: ## Typecheck the ui
	cd ui && npx tsc --noEmit

.PHONY: verify
verify: lint test ## Everything short of e2e

## --- e2e ---

.PHONY: e2e
e2e: ## Full e2e suite (FerretDB + kind + Core + Periphery + tests)
	scripts/e2e.sh

.PHONY: e2e-up
e2e-up: ## Bring the e2e stack up and leave it running
	scripts/e2e.sh up

.PHONY: e2e-test
e2e-test: ## Run e2e tests against an already-running stack
	scripts/e2e.sh test

.PHONY: e2e-down
e2e-down: ## Tear the e2e stack down (incl. the kind cluster)
	scripts/e2e.sh down

## --- docker ---

# Networks that intercept TLS (Fortinet, Zscaler) break cargo/yarn/deno
# inside the build, since the base images only trust public roots. Any
# host CA matching this glob is copied into the build's cert directory.
# Nothing matches on an unintercepted network, and the builds are
# unchanged.
HOST_CA_GLOB ?= /usr/local/share/ca-certificates/*.crt
CA_DIR := docker/ca-certificates

# Container egress: crates.io is merely intercepted (the CA above is
# enough), but the npm registries are unreachable without the proxy, and
# the proxy will not serve registry.yarnpkg.com. Both are forwarded only
# when a proxy is actually set in the environment, so an unproxied
# machine builds exactly as upstream does.
PROXY := $(or $(https_proxy),$(HTTPS_PROXY))
ifneq ($(PROXY),)
BUILD_ARGS := --build-arg HTTPS_PROXY=$(PROXY) --build-arg HTTP_PROXY=$(PROXY) \
  --build-arg NO_PROXY="$(NO_PROXY)" \
  --build-arg YARN_REGISTRY=$(REGISTRY)
endif

.PHONY: docker-ca
docker-ca: ## Copy host CA certificates into the docker build (for TLS interception)
	@shopt -s nullglob; certs=($(HOST_CA_GLOB)); \
	if [ $${#certs[@]} -eq 0 ]; then \
	  echo "no host CAs matched $(HOST_CA_GLOB); nothing to do"; \
	else \
	  cp "$${certs[@]}" $(CA_DIR)/ && \
	  echo "copied $${#certs[@]} CA certificate(s) into $(CA_DIR)/"; \
	fi

.PHONY: compose-up
compose-up: compose-build ## Dev stack in docker, Core exposed on :9120
	docker compose -p komodo-dev -f dev.compose.yaml -f expose.compose.yaml up -d

.PHONY: compose-build
compose-build: docker-ca ## Build the dev compose images
	docker compose -p komodo-dev -f dev.compose.yaml build $(BUILD_ARGS)

.PHONY: compose-down
compose-down: ## Tear the dev compose stack down
	docker compose -p komodo-dev -f dev.compose.yaml down --remove-orphans

## --- registry ---

# Fork images for the internal Harbor. Both core and periphery are built:
# compose.yml on the deploy host derives both image lines from one tag
# variable, so shipping only one of them leaves the other pointing at a
# tag that exists in neither registry.
HARBOR_REPO ?= harbor.vici.corp/datateam/komodo
IMAGES ?= core periphery
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
SHA := $(shell git rev-parse --short HEAD)
# Pin deploys to TAG. MOVING_TAG is a convenience alias that the next
# build reassigns, so it names different bytes over time.
TAG ?= $(VERSION)-k8s-$(SHA)
MOVING_TAG ?= $(VERSION)-k8s

.PHONY: docker-push
docker-push: docker-ca ## Build + push core and periphery images to Harbor (ALLOW_DIRTY=1 skips the clean-tree gate)
	@if [ -z "$(ALLOW_DIRTY)" ] && [ -n "$$(git status --porcelain)" ]; then \
	  echo "refusing to push: working tree is dirty, so :$(TAG) would not name the bytes" >&2; \
	  echo "the sha claims. Commit first, or re-run with ALLOW_DIRTY=1." >&2; \
	  exit 1; \
	fi
	@set -e; for img in $(IMAGES); do \
	  echo "==> build $(HARBOR_REPO)/$$img:$(TAG)"; \
	  docker build -f bin/$$img/aio.Dockerfile $(BUILD_ARGS) \
	    -t $(HARBOR_REPO)/$$img:$(TAG) \
	    -t $(HARBOR_REPO)/$$img:$(MOVING_TAG) .; \
	done
	@set -e; for img in $(IMAGES); do \
	  echo "==> push $(HARBOR_REPO)/$$img"; \
	  docker push $(HARBOR_REPO)/$$img:$(TAG); \
	  docker push $(HARBOR_REPO)/$$img:$(MOVING_TAG); \
	done
	@echo; echo "pin these on the deploy host:"; \
	for img in $(IMAGES); do echo "  $(HARBOR_REPO)/$$img:$(TAG)"; done

## --- remote build ---

# The workstation does not have the disk for this build: target/ alone
# peaks past 20G. data-backend-01 has 16 cores and ~300G free, so the
# build and the push both happen there and the image never travels
# through this machine.
#
# rsync rather than DOCKER_HOST=ssh://, so an iteration ships only what
# changed instead of re-streaming a ~540M context every time. .git goes
# along (78M, and incremental after the first sync) specifically so the
# remote runs docker-push's own clean-tree gate against a real repo,
# rather than trusting a tag this end computed.
BUILD_HOST ?= data-backend-01
BUILD_PATH ?= build/komodo
# data-backend-01 reaches registry.npmjs.org directly but gets 403 from
# both static.crates.io and registry.yarnpkg.com, so the build needs the
# proxy exactly like this machine does — an unproxied remote build fails
# in cargo fetch. NO_PROXY keeps the Harbor push off the proxy.
BUILD_PROXY ?= $(or $(https_proxy),$(HTTPS_PROXY),http://172.21.10.22:8888/)
BUILD_NO_PROXY ?= 172.21.0.0/16,.viciholdings.com,.vici.corp,.vidi.com,localhost,127.0.0.1,10.0.0.0/8,172.16.0.0/20,192.168.0.0/16

.PHONY: remote-build
remote-build: ## Build + push the Harbor images on BUILD_HOST rather than locally
	@echo "==> syncing to $(BUILD_HOST):$(BUILD_PATH)"
	@ssh $(BUILD_HOST) 'mkdir -p $(BUILD_PATH)'
	rsync -az --delete \
	  --exclude 'target/' --exclude 'node_modules/' --exclude '.dev/' \
	  ./ $(BUILD_HOST):$(BUILD_PATH)/
	@echo "==> building on $(BUILD_HOST)"
	ssh $(BUILD_HOST) 'cd $(BUILD_PATH) && \
	  https_proxy=$(BUILD_PROXY) HTTPS_PROXY=$(BUILD_PROXY) \
	  http_proxy=$(BUILD_PROXY) HTTP_PROXY=$(BUILD_PROXY) \
	  NO_PROXY=$(BUILD_NO_PROXY) no_proxy=$(BUILD_NO_PROXY) \
	  make docker-push $(if $(ALLOW_DIRTY),ALLOW_DIRTY=1,)'
