.DEFAULT_GOAL := help

.PHONY: help
help: # Show help for each of the Makefile recipes.
	@grep -E '^[a-zA-Z0-9 -]+:.*#'  Makefile | sort | while read -r l; do printf "\033[1;32m$$(echo $$l | cut -f 1 -d':')\033[00m:$$(echo $$l | cut -f 2- -d'#')\n"; done

AMM_CONTRACTS = ./contracts
AMM_CONTRACTS_PATHS := $(shell find $(AMM_CONTRACTS) -mindepth 1 -maxdepth 1 -type d)

CONTRACTS := factory_contract pair_contract router_contract router_v2_contract stable_pool_contract mock_rate_provider_contract flashswap_contract

INK_DEV_IMAGE := "public.ecr.aws/p6e8q1z1/ink-dev:2.1.0"
SCRIPT_DIR := $(shell cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

.PHONY: check-amm
check-amm: check-drink-tests ## Runs cargo (contract) check on AMM contracts.
	@for d in $(AMM_CONTRACTS_PATHS); do \
		echo "Checking $$d" ; \
		cargo check --quiet --all-targets --all-features --manifest-path $$d/Cargo.toml ; \
		cargo clippy --quiet --all-features --manifest-path $$d/Cargo.toml -- --no-deps -D warnings ; \
		cargo contract check --quiet --manifest-path $$d/Cargo.toml ; \
	done

.PHONY: build-all
build-all: ## Builds all contracts.
	@for d in $(AMM_CONTRACTS_PATHS); do \
	 	echo "Building $$d contract" ; \
	 	cargo contract build --quiet --manifest-path $$d/Cargo.toml --release ; \
	done
	@for d in $(CONTRACTS); do \
		cp ../target/ink/$$d/$$d.wasm ../artifacts/$$d.wasm ; \
		cp ../target/ink/$$d/$$d.json ../artifacts/$$d.json ; \
		cp ../target/ink/$$d/$$d.contract ../artifacts/$$d.contract ; \
	done

.PHONY: wrap-all
wrap-all: ## Generates Rust wrappers for interacting with AMM contracts.
	./scripts/prepare_rust_wrappers.sh

.PHONY: wrap-all-dockerized
wrap-all-dockerized: ## Generates Rust wrappers for interacting with AMM contracts using Docker.
	@docker run --rm \
		--name ink-dev \
		-v "$(SCRIPT_DIR)/..":/code \
		--workdir /code/amm \
		$(INK_DEV_IMAGE) \
		make wrap-all

.PHONY: setup-tests
setup-tests: build-all wrap-all  ## Builds contracts and generates wrappers.

.PHONY: drink-tests
drink-tests: ## Runs tests for drink contract.
	@echo "Running drink tests" ; \
	cargo test --manifest-path ./drink-tests/Cargo.toml -- --nocapture ; \

.PHONY: check-drink-tests
check-drink-tests: ## Runs cargo checks on drink tests.
	@echo "Checking drink tests" ; \
	cargo check --tests --manifest-path ./drink-tests/Cargo.toml ; \

.PHONY:
all-drink: setup-tests drink-tests ## Runs all tests.
