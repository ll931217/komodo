#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

git_command() {
  printf '%q ' "$@"
  printf '\n'
}

assert_git_log() {
  local actual=$1
  local expected=$2
  cmp -s "$expected" "$actual" \
    || fail "git command log differed\nexpected:\n$(<"$expected")actual:\n$(<"$actual")"
}

run_case() {
  local shape=$1
  local expected_status=$2
  local expected_message=$3
  local commit_message
  printf -v commit_message '%s\n\n%s\n%s' \
    'chore(komodo): move Core and Periphery onto the 2.3.2-k8s images' \
    "Automated by the komodo fork's \`make deploy\` on a version change. Both" \
    'image lines move together; only `core` is deployed from Komodo.'
  local tmp output status git_log expected_git_log
  tmp=$(mktemp -d)
  trap 'if [ -n "${tmp:-}" ] && [ -d "$tmp" ]; then rm -rf -- "$tmp"; fi' RETURN EXIT
  git_log=$tmp/git.log
  expected_git_log=$tmp/expected-git.log
  : > "$git_log"
  : > "$expected_git_log"
  mkdir -p "$tmp/infra/.git" "$tmp/bin"

  case "$shape" in
    zero)
      printf 'services:\n  mongo:\n    image: mongo:8\n' > "$tmp/infra/compose.yml"
      ;;
    one)
      printf 'services:\n  core:\n    image: registry.invalid/core:2.3.2-k8s\n' > "$tmp/infra/compose.yml"
      ;;
    only-periphery)
      printf 'services:\n  periphery:\n    image: registry.invalid/periphery:2.3.2-k8s\n' > "$tmp/infra/compose.yml"
      ;;
    duplicate-core)
      printf 'services:\n  core-a:\n    image: registry.invalid/core:2.3.2-k8s\n  core-b:\n    image: registry.invalid/core:2.3.2-k8s\n  periphery:\n    image: registry.invalid/periphery:2.3.2-k8s\n' > "$tmp/infra/compose.yml"
      ;;
    duplicate-periphery)
      printf 'services:\n  core:\n    image: registry.invalid/core:2.3.2-k8s\n  periphery-a:\n    image: registry.invalid/periphery:2.3.2-k8s\n  periphery-b:\n    image: registry.invalid/periphery:2.3.2-k8s\n' > "$tmp/infra/compose.yml"
      ;;
    matching)
      printf 'services:\n  core:\n    image: registry.invalid/core:2.3.2-k8s\n  periphery:\n    image: registry.invalid/periphery:2.3.2-k8s\n' > "$tmp/infra/compose.yml"
      ;;
    stale)
      printf 'services:\n  core:\n    image: registry.invalid/core:old\n  periphery:\n    image: registry.invalid/periphery:old\n' > "$tmp/infra/compose.yml"
      ;;
    regex-metachar-stale)
      printf 'services:\n  core:\n    image: registry.invalid/core:2x3y2-k8s\n  periphery:\n    image: registry.invalid/periphery:2x3y2-k8s\n' > "$tmp/infra/compose.yml"
      ;;
    *) fail "unknown test shape: $shape" ;;
  esac

  cat > "$tmp/bin/ssh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [ "${FORBID_REMOTE:-0}" = 1 ]; then
  printf 'BUG: reached forbidden remote stub\n' >&2
  exit 42
fi
printf 'sha256:expected\n'
SH

  cat > "$tmp/bin/curl" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [ "${FORBID_REMOTE:-0}" = 1 ]; then
  printf 'BUG: reached forbidden curl stub\n' >&2
  exit 42
fi
case "$*" in
  *'/read/GetStack'*) printf '{"config":{"server_id":"server"}}\n' ;;
  *'/read/InspectContainer'*) printf '{"Image":"sha256:expected"}\n' ;;
  *'/read/GetVersion'*) printf '{"version":"2.3.2"}\n' ;;
  *'/execute/DeployStack'*) printf '{}\n' ;;
  *) printf 'unexpected curl endpoint\n' >&2; exit 43 ;;
esac
SH

  cat > "$tmp/bin/git" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%q ' "$@" >> "$GIT_LOG"
printf '\n' >> "$GIT_LOG"
if [ "${FORBID_REMOTE:-0}" = 1 ]; then
  printf 'BUG: reached forbidden git stub\n' >&2
  exit 42
fi
case "$*" in
  *' diff --quiet'*) exit 0 ;;
  *' diff --cached --quiet'*) exit 0 ;;
  *' add compose.yml'|*' commit '*|*' push '* ) exit 0 ;;
  *' rev-parse HEAD'*) printf 'commitsha\n' ;;
  *' rev-parse --abbrev-ref HEAD'*) printf 'main\n' ;;
  *' ls-remote '* ) printf 'commitsha\trefs/heads/main\n' ;;
  *) printf 'unexpected git command: %s\n' "$*" >&2; exit 44 ;;
esac
SH
  chmod +x "$tmp/bin"/*

  set +e
  output=$(PATH="$tmp/bin:$PATH" FORBID_REMOTE=$([ "$expected_status" -ne 0 ] && printf 1 || printf 0) \
    GIT_LOG="$git_log" KOMODO_API_KEY=test-key KOMODO_API_SECRET=test-secret \
    "$ROOT/scripts/deploy-core.sh" 2.3.2 2.3.2-k8s-deadbeef 2.3.2-k8s \
    registry.invalid build-host "$tmp/infra" 2>&1)
  status=$?
  set -e

  [ "$status" -eq "$expected_status" ] || fail "$shape returned $status, expected $expected_status\n$output"
  case "$output" in
    *"$expected_message"*) ;;
    *) fail "$shape output missed '$expected_message'\n$output" ;;
  esac

  case "$shape" in
    zero|one|only-periphery|duplicate-core|duplicate-periphery)
      [ ! -s "$git_log" ] || fail "$shape invoked git: $(<"$git_log")"
      ;;
    matching)
      : > "$expected_git_log"
      assert_git_log "$git_log" "$expected_git_log"
      ;;
    stale|regex-metachar-stale)
      {
        git_command -C "$tmp/infra" diff --quiet
        git_command -C "$tmp/infra" diff --cached --quiet
        git_command -C "$tmp/infra" add compose.yml
        git_command -C "$tmp/infra" commit -q -m "$commit_message"
        git_command -C "$tmp/infra" push -q origin HEAD
        git_command -C "$tmp/infra" rev-parse HEAD
        git_command -C "$tmp/infra" rev-parse --abbrev-ref HEAD
        git_command -C "$tmp/infra" ls-remote origin -h refs/heads/main
      } > "$expected_git_log"
      assert_git_log "$git_log" "$expected_git_log"
      ;;
  esac

  if [ "$shape" = stale ] || [ "$shape" = regex-metachar-stale ]; then
    grep -Fq 'image: registry.invalid/core:2.3.2-k8s' "$tmp/infra/compose.yml" \
      || fail 'stale core line was not rewritten'
    grep -Fq 'image: registry.invalid/periphery:2.3.2-k8s' "$tmp/infra/compose.yml" \
      || fail 'stale periphery line was not rewritten'
  fi
}

run_case zero 1 'exactly one Core and one Periphery image line'
run_case one 1 'exactly one Core and one Periphery image line'
run_case only-periphery 1 'exactly one Core and one Periphery image line'
run_case duplicate-core 1 'exactly one Core and one Periphery image line'
run_case duplicate-periphery 1 'exactly one Core and one Periphery image line'
run_case matching 0 'core is on sha256:expected and reports 2.3.2'
run_case stale 0 'core is on sha256:expected and reports 2.3.2'
run_case regex-metachar-stale 0 'core is on sha256:expected and reports 2.3.2'
printf 'PASS: deploy-core shape regression tests\n'
