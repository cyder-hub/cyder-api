default: list

# Show available local shortcuts.
list:
	@just --list

# Run backend and frontend dev servers together.
dev:
	#!/usr/bin/env bash
	set -euo pipefail

	backend_pid=""
	frontend_pid=""
	started_pid=""

	start_recipe() {
	  local recipe="$1"
	  if command -v setsid >/dev/null 2>&1; then
	    setsid just "$recipe" &
	  else
	    just "$recipe" &
	  fi
	  started_pid="$!"
	}

	stop_process() {
	  local pid="$1"
	  [[ -n "$pid" ]] || return 0

	  if kill -0 "-$pid" 2>/dev/null; then
	    kill -TERM "-$pid" 2>/dev/null || true
	  elif kill -0 "$pid" 2>/dev/null; then
	    kill -TERM "$pid" 2>/dev/null || true
	  fi
	}

	cleanup() {
	  local status="$?"
	  trap - INT TERM EXIT
	  stop_process "$backend_pid"
	  stop_process "$frontend_pid"
	  wait "$backend_pid" 2>/dev/null || true
	  wait "$frontend_pid" 2>/dev/null || true
	  exit "$status"
	}

	trap cleanup INT TERM EXIT

	start_recipe dev-backend
	backend_pid="$started_pid"
	start_recipe dev-front
	frontend_pid="$started_pid"

	while true; do
	  if ! kill -0 "$backend_pid" 2>/dev/null; then
	    set +e
	    wait "$backend_pid"
	    status="$?"
	    set -e
	    exit "$status"
	  fi

	  if ! kill -0 "$frontend_pid" 2>/dev/null; then
	    set +e
	    wait "$frontend_pid"
	    status="$?"
	    set -e
	    exit "$status"
	  fi

	  sleep 1
	done

# Run the backend dev server.
dev-backend:
	#!/usr/bin/env bash
	set -euo pipefail
	if [[ -z "${CYDER_DATA_DIR:-}" && -z "${CYDER_CONFIG_PATH:-}" ]]; then
	  export CYDER_DATA_DIR='{{justfile_directory()}}/.cyder/dev'
	fi
	exec cargo run -p cyder-api

# Ensure frontend deps and run the Vite dev server.
dev-front: install-front-deps
	npm --prefix front run dev

# Ensure frontend dependencies for iterative development.
install-front-deps:
	#!/usr/bin/env bash
	set -euo pipefail
	marker="front/node_modules/.package-lock.json"
	if [[ ! -f "$marker" || front/package.json -nt "$marker" || front/package-lock.json -nt "$marker" ]]; then
	  npm --prefix front install
	fi

# Install locked frontend dependencies for verification/builds.
front-ci-deps:
	npm --prefix front ci

# Build backend and frontend release artifacts.
build: build-backend build-front

# Build the backend release binary.
build-backend:
	cargo build -p cyder-api --release

# Build frontend assets from locked dependencies.
build-front: front-ci-deps
	npm --prefix front run build

# Run backend and frontend tests.
test: test-backend test-front

# Run backend tests.
test-backend:
	cargo test -p cyder-api

# Run frontend tests.
test-front:
	npm --prefix front test

# Run the local aggregate verification suite.
check: fmt-check log-lint test-backend front-ci-deps i18n-check test-front
	npm --prefix front run build

# Format Rust sources.
fmt:
	cargo fmt

# Check Rust formatting without writing changes.
fmt-check:
	cargo fmt --check

# Run backend log lint.
log-lint:
	cargo run -p cyder-api --bin log_lint

# Check frontend i18n coverage.
i18n-check:
	npm --prefix front run i18n:check

# Run the quick transform quality gate.
transform-gate:
	cargo run -p cyder-api --bin transform_quality_gate -- --quick

# Run the quick transform quality gate and write a JSON report.
transform-gate-report report="/tmp/transform-quality-report.json":
	cargo run -p cyder-api --bin transform_quality_gate -- --quick --report-out "{{report}}"
