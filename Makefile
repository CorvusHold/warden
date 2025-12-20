DOCKER_COMPOSE_CI := tests/docker-compose.ci.yml

CI_POSTGRES_USER ?= warden_dev
CI_POSTGRES_PASSWORD ?= warden_dev
CI_POSTGRES_DB ?= warden_dev
CI_POSTGRES_PORT ?= 5432

CI_AWS_ACCESS_KEY_ID ?= minioadmin
CI_AWS_SECRET_ACCESS_KEY ?= minioadmin
CI_AWS_ENDPOINT ?= http://localhost:9000
CI_AWS_CONSOLE_ENDPOINT ?= http://localhost:9001
CI_AWS_REGION ?= us-east-1
CI_AWS_TEST_BUCKET ?= testbucket

CI_DOCKER_HOST ?= unix:///var/run/docker.sock
CI_TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE ?= /var/run/docker.sock
CI_TESTCONTAINERS_RYUK_DISABLED ?= true

define CI_ENV_LINT
 CARGO_TERM_COLOR=always \
 POSTGRES_USER=$(CI_POSTGRES_USER) \
 POSTGRES_PASSWORD=$(CI_POSTGRES_PASSWORD) \
 POSTGRES_DB=$(CI_POSTGRES_DB) \
 POSTGRES_PORT=$(CI_POSTGRES_PORT) \
 AWS_ACCESS_KEY_ID=$(CI_AWS_ACCESS_KEY_ID) \
 AWS_SECRET_ACCESS_KEY=$(CI_AWS_SECRET_ACCESS_KEY) \
 AWS_ENDPOINT=$(CI_AWS_ENDPOINT) \
 AWS_CONSOLE_ENDPOINT=$(CI_AWS_CONSOLE_ENDPOINT) \
 AWS_REGION=$(CI_AWS_REGION) \
 AWS_TEST_BUCKET=$(CI_AWS_TEST_BUCKET) \
 CI=true \
 
endef

define CI_ENV_TEST
 DOCKER_HOST=$(CI_DOCKER_HOST) \
 TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE=$(CI_TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE) \
 TESTCONTAINERS_RYUK_DISABLED=$(CI_TESTCONTAINERS_RYUK_DISABLED) \
 POSTGRES_USER=$(CI_POSTGRES_USER) \
 POSTGRES_PASSWORD=$(CI_POSTGRES_PASSWORD) \
 POSTGRES_DB=$(CI_POSTGRES_DB) \
 POSTGRES_PORT=$(CI_POSTGRES_PORT) \
 AWS_ACCESS_KEY_ID=$(CI_AWS_ACCESS_KEY_ID) \
 AWS_SECRET_ACCESS_KEY=$(CI_AWS_SECRET_ACCESS_KEY) \
 AWS_ENDPOINT=$(CI_AWS_ENDPOINT) \
 AWS_CONSOLE_ENDPOINT=$(CI_AWS_CONSOLE_ENDPOINT) \
 AWS_REGION=$(CI_AWS_REGION) \
 AWS_TEST_BUCKET=$(CI_AWS_TEST_BUCKET) \
 CI=true \
 
endef

.PHONY: ci-up ci-down docker-info lint-ci test-ci gha-lint gha-tests gha act-lint act-tests act ci test-e2e test-errors chaos-test test-ha-failures chaos-test-all chaos-test-quick

ci-up:
	docker compose -f $(DOCKER_COMPOSE_CI) up -d

ci-down:
	docker compose -f $(DOCKER_COMPOSE_CI) down

docker-info:
	docker info

lint-ci: ci-up
	@set -e; trap '$(MAKE) ci-down' EXIT; \
	$(CI_ENV_LINT) cargo fmt -- --check; \
	$(CI_ENV_LINT) cargo clippy --workspace -- -D warnings

# Run the full test suite in a CI-like environment (MinIO + Testcontainers)
test-ci: ci-up docker-info
	@set -e; trap '$(MAKE) ci-down' EXIT; \
	$(CI_ENV_TEST) cargo test --workspace -- --test-threads=1

gha-lint: lint-ci

gha-tests: test-ci

gha: gha-lint gha-tests

act-lint:
	act -j lint

act-tests:
	act -j tests

act: act-lint act-tests

# Run E2E scenario tests (requires Docker + MinIO)
test-e2e: ci-up
	@set -e; trap '$(MAKE) ci-down' EXIT; \
	$(CI_ENV_TEST) cargo test -p postgres --test e2e_scenarios_test -- --ignored --test-threads=1

# Run error handling tests
test-errors:
	cargo test -p postgres --test error_handling_test -- --test-threads=1

# Run chaos/failure tests (requires Docker + MinIO + PostgreSQL)
chaos-test: ci-up
	@set -e; trap '$(MAKE) ci-down' EXIT; \
	echo "Running chaos and failure mode tests..."; \
	$(CI_ENV_TEST) POSTGRES_HOST=localhost cargo test -p postgres --test chaos_test -- --ignored --test-threads=1

# Run HA failure mode tests
test-ha-failures:
	cargo test -p postgres --test ha_failure_test -- --test-threads=1

# Run all chaos tests (unit + integration)
chaos-test-all: test-ha-failures chaos-test
	@echo "All chaos tests completed"

# Run quick chaos tests (unit tests only, no external dependencies)
chaos-test-quick:
	cargo test -p postgres --test chaos_test -- --test-threads=1
	cargo test -p postgres --test ha_failure_test -- --test-threads=1

ci: lint-ci test-ci ci-down