#!/usr/bin/env bash
# End to end test harness: FerretDB (docker) + kind cluster + Core and
# Periphery as host processes, then `cargo test -p komodo_e2e`.
#
# Usage:
#   scripts/e2e.sh            # full cycle: up, test, down
#   scripts/e2e.sh up         # start the stack, leave it running
#   scripts/e2e.sh test       # run tests against a running stack
#   scripts/e2e.sh down       # stop everything
#
# KOMODO_E2E_KEEP_KIND=1 keeps the kind cluster across runs (faster locally).
#
# Ports are deliberately offset by one from the Komodo defaults
# (9120/8120/27017) so this stack can run alongside a normal dev stack.

set -euo pipefail

cd "$(dirname "$0")/.."

STATE_DIR="$PWD/e2e/.state"
COMPOSE="docker compose -p komodo-e2e -f e2e/compose.yaml"
KIND_CLUSTER="komodo-e2e"
CORE_PORT=9121
PERIPHERY_PORT=8121

export KOMODO_ADDRESS="http://localhost:$CORE_PORT"
export KOMODO_E2E_USERNAME="e2e-admin"
export KOMODO_E2E_PASSWORD="e2e-password"

# Behind a corporate proxy, the test client would route localhost
# through it and fail to reach Core.
export no_proxy="localhost,127.0.0.1,::1${no_proxy:+,$no_proxy}"
export NO_PROXY="$no_proxy"

mkdir -p "$STATE_DIR"/{keys,syncs,repo-cache,action-cache,periphery}

# Serialize runs: concurrent invocations share ports, state dir, compose
# project and kind cluster, so one run's teardown would kill the other's.
exec 9>"$STATE_DIR/lock"
if ! flock -n 9; then
  echo "Another scripts/e2e.sh run holds $STATE_DIR/lock" >&2
  exit 1
fi

# Wait for a pid to exit, then force kill if it outlives the grace period.
stop_pid() {
  local pid_file="$1" pid
  [ -f "$pid_file" ] || return 0
  pid=$(cat "$pid_file")
  kill "$pid" 2>/dev/null || true
  for _ in $(seq 1 50); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.2
  done
  kill -9 "$pid" 2>/dev/null || true
  rm -f "$pid_file"
}

assert_not_running() {
  local pid_file="$1" name="$2"
  if [ -f "$pid_file" ] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
    echo "$name is already running (pid $(cat "$pid_file")). Run 'scripts/e2e.sh down' first." >&2
    exit 1
  fi
}

# Poll a host port until it accepts a connection.
wait_for_port() {
  local port="$1" name="$2"
  for _ in $(seq 1 60); do
    if (echo >"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  echo "$name did not open port $port" >&2
  return 1
}

up() {
  assert_not_running "$STATE_DIR/core.pid" Core
  assert_not_running "$STATE_DIR/periphery.pid" Periphery

  $COMPOSE up -d
  wait_for_port 27018 FerretDB

  if ! kind get clusters 2>/dev/null | grep -qx "$KIND_CLUSTER"; then
    kind create cluster --name "$KIND_CLUSTER" --wait 120s
  fi
  kind export kubeconfig --name "$KIND_CLUSTER" \
    --kubeconfig "$STATE_DIR/kubeconfig"

  cargo build -p komodo_core -p komodo_periphery

  PERIPHERY_ROOT_DIRECTORY="$STATE_DIR/periphery" \
  PERIPHERY_SSL_ENABLED=false \
  PERIPHERY_PORT="$PERIPHERY_PORT" \
    ./target/debug/periphery -c config/periphery.config.toml \
    >"$STATE_DIR/periphery.log" 2>&1 &
  echo $! >"$STATE_DIR/periphery.pid"

  KOMODO_CONFIG_PATH=config/core.config.toml \
  KOMODO_PORT="$CORE_PORT" \
  KOMODO_DATABASE_ADDRESS=localhost:27018 \
  KOMODO_JWT_SECRET=e2e-jwt-secret \
  KOMODO_LOCAL_AUTH=true \
  KOMODO_INIT_ADMIN_USERNAME="$KOMODO_E2E_USERNAME" \
  KOMODO_INIT_ADMIN_PASSWORD="$KOMODO_E2E_PASSWORD" \
  KOMODO_FIRST_SERVER="http://localhost:$PERIPHERY_PORT" \
  KOMODO_PRIVATE_KEY="file:$STATE_DIR/keys/core.key" \
  KOMODO_SYNC_DIRECTORY="$STATE_DIR/syncs" \
  KOMODO_REPO_DIRECTORY="$STATE_DIR/repo-cache" \
  KOMODO_ACTION_DIRECTORY="$STATE_DIR/action-cache" \
    ./target/debug/core \
    >"$STATE_DIR/core.log" 2>&1 &
  echo $! >"$STATE_DIR/core.pid"

  echo "Waiting for Core at $KOMODO_ADDRESS ..."
  for _ in $(seq 1 60); do
    if curl -sf -o /dev/null --noproxy '*' \
      "$KOMODO_ADDRESS/auth/version"; then
      echo "Core is up."
      return 0
    fi
    sleep 1
  done
  echo "Core did not come up. Last log lines:" >&2
  tail -40 "$STATE_DIR/core.log" >&2
  return 1
}

run_tests() {
  cargo test -p komodo_e2e -- --nocapture
}

down() {
  stop_pid "$STATE_DIR/core.pid"
  stop_pid "$STATE_DIR/periphery.pid"
  $COMPOSE down --remove-orphans || true
  if [ "${KOMODO_E2E_KEEP_KIND:-0}" != "1" ]; then
    kind delete cluster --name "$KIND_CLUSTER" 2>/dev/null || true
  fi
}

case "${1:-all}" in
  up) up ;;
  test) run_tests ;;
  down) down ;;
  all)
    trap down EXIT
    up
    run_tests
    ;;
  *)
    echo "Usage: $0 [up|test|down]" >&2
    exit 1
    ;;
esac
