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

.PHONY: compose-up
compose-up: ## Dev stack in docker, Core exposed on :9120
	docker compose -p komodo-dev -f dev.compose.yaml -f expose.compose.yaml up -d

.PHONY: compose-build
compose-build: ## Build the dev compose images
	docker compose -p komodo-dev -f dev.compose.yaml build

.PHONY: compose-down
compose-down: ## Tear the dev compose stack down
	docker compose -p komodo-dev -f dev.compose.yaml down --remove-orphans
