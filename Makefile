DOCKER_COMPOSE_CI := tests/docker-compose.ci.yml

.PHONY: ci-up ci-down lint-ci test-ci

ci-up:
	docker compose -f $(DOCKER_COMPOSE_CI) up -d

ci-down:
	docker compose -f $(DOCKER_COMPOSE_CI) down

lint-ci: ci-up
	CARGO_TERM_COLOR=always \
	POSTGRES_USER=warden_dev \
	POSTGRES_PASSWORD=warden_dev \
	POSTGRES_DB=warden_dev \
	POSTGRES_PORT=5432 \
	AWS_ACCESS_KEY_ID=minioadmin \
	AWS_SECRET_ACCESS_KEY=minioadmin \
	AWS_ENDPOINT=http://127.0.0.1:9000 \
	AWS_CONSOLE_ENDPOINT=http://127.0.0.1:9001 \
	AWS_TEST_BUCKET=testbucket \
	CI=true \
	cargo fmt -- --check
	CARGO_TERM_COLOR=always \
	POSTGRES_USER=warden_dev \
	POSTGRES_PASSWORD=warden_dev \
	POSTGRES_DB=warden_dev \
	POSTGRES_PORT=5432 \
	AWS_ACCESS_KEY_ID=minioadmin \
	AWS_SECRET_ACCESS_KEY=minioadmin \
	AWS_ENDPOINT=http://localhost:9000 \
	AWS_CONSOLE_ENDPOINT=http://localhost:9001 \
	AWS_TEST_BUCKET=testbucket \
	CI=true \
	cargo clippy --workspace -- -D warnings

# Run the full test suite in a CI-like environment (MinIO + Testcontainers)
test-ci: ci-up
	DOCKER_HOST=unix:///var/run/docker.sock \
	TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE=/var/run/docker.sock \
	TESTCONTAINERS_RYUK_DISABLED=true \
	AWS_ACCESS_KEY_ID=minioadmin \
	AWS_SECRET_ACCESS_KEY=minioadmin \
	AWS_ENDPOINT=http://127.0.0.1:9000 \
	AWS_CONSOLE_ENDPOINT=http://127.0.0.1:9001 \
	AWS_TEST_BUCKET=testbucket \
	CI=true \
	cargo test --workspace -- --test-threads=1

# Run E2E scenario tests (requires Docker + MinIO)
test-e2e: ci-up
	DOCKER_HOST=unix:///var/run/docker.sock \
	TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE=/var/run/docker.sock \
	TESTCONTAINERS_RYUK_DISABLED=true \
	POSTGRES_USER=warden_dev \
	POSTGRES_PASSWORD=warden_dev \
	POSTGRES_DB=warden_dev \
	POSTGRES_PORT=5432 \
	AWS_ACCESS_KEY_ID=minioadmin \
	AWS_SECRET_ACCESS_KEY=minioadmin \
	AWS_ENDPOINT=http://127.0.0.1:9000 \
	AWS_REGION=us-east-1 \
	AWS_TEST_BUCKET=testbucket \
	CI=true \
	cargo test -p postgres --test e2e_scenarios_test -- --ignored --test-threads=1

# Run error handling tests
test-errors:
	cargo test -p postgres --test error_handling_test -- --test-threads=1

# Run chaos/failure tests (requires Docker + MinIO + PostgreSQL)
chaos-test: ci-up
	@echo "Running chaos and failure mode tests..."
	DOCKER_HOST=unix:///var/run/docker.sock \
	TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE=/var/run/docker.sock \
	TESTCONTAINERS_RYUK_DISABLED=true \
	POSTGRES_USER=warden_dev \
	POSTGRES_PASSWORD=warden_dev \
	POSTGRES_DB=warden_dev \
	POSTGRES_HOST=localhost \
	POSTGRES_PORT=5432 \
	AWS_ACCESS_KEY_ID=minioadmin \
	AWS_SECRET_ACCESS_KEY=minioadmin \
	AWS_ENDPOINT=http://127.0.0.1:9000 \
	AWS_REGION=us-east-1 \
	AWS_TEST_BUCKET=testbucket \
	CI=true \
	cargo test -p postgres --test chaos_test -- --ignored --test-threads=1

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