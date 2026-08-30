#!/usr/bin/env bash
# Build the fork images, then deploy Core from the Komodo Stack that manages it.
#
# Driven by `make deploy`. Split out of the Makefile because the version
# reconciliation and the digest read-back are too much logic for recipe lines.
#
# What this deliberately does NOT do: deploy the `periphery` service. That
# request would be executed BY the agent it replaces, which kills the deploy
# mid-flight. Agents are their own rollout (upgrade-periphery.sh), and the
# deploy host's own agent is recreated over plain ssh with --no-deps.
set -euo pipefail

VERSION="$1"           # workspace version, from the Makefile
TAG="$2"               # <version>-k8s-<sha>, the immutable tag
MOVING_TAG="$3"        # <version>-k8s, what compose.yml names
HARBOR_REPO="$4"
BUILD_HOST="$5"
INFRA_PATH="$6"
KOMODO_HOST="${KOMODO_API_URL:-https://komodo.data.vici.corp}"
STACK="${KOMODO_STACK:-komodo}"

: "${KOMODO_API_KEY:?set KOMODO_API_KEY}"
: "${KOMODO_API_SECRET:?set KOMODO_API_SECRET}"

api() {
  curl -sS --fail-with-body --max-time 300 -X POST "$KOMODO_HOST$1" \
    -H "X-API-KEY: $KOMODO_API_KEY" -H "X-API-SECRET: $KOMODO_API_SECRET" \
    -H 'Content-Type: application/json' -d "$2"
}

say() { printf '==> %s\n' "$1"; }
die() { printf 'deploy aborted: %s\n' "$1" >&2; exit 1; }

# --- 1. reconcile the infra repo tag -----------------------------------------
# compose.yml names the MOVING tag, so a same-version rebuild needs no edit at
# all - the alias already points at the new bytes and the Stack has
# auto_pull. Only a VERSION change renames the tag, and only then is a commit
# warranted. Bumping unconditionally would land an empty commit every build.
[ -d "$INFRA_PATH/.git" ] || die "$INFRA_PATH is not a git checkout"

infra_tag_line() { grep -nE "^[[:space:]]*image:[[:space:]]*\S*/(core|periphery):" "$INFRA_PATH/compose.yml"; }
core_lines=$(grep -nE "^[[:space:]]*image:[[:space:]]*\S*/core:" "$INFRA_PATH/compose.yml" || true)
periphery_lines=$(grep -nE "^[[:space:]]*image:[[:space:]]*\S*/periphery:" "$INFRA_PATH/compose.yml" || true)
core_count=$(printf '%s\n' "$core_lines" | sed '/^$/d' | wc -l)
periphery_count=$(printf '%s\n' "$periphery_lines" | sed '/^$/d' | wc -l)
[ "$core_count" -eq 1 ] && [ "$periphery_count" -eq 1 ] \
  || die "compose.yml must contain exactly one Core and one Periphery image line (found Core $core_count, Periphery $periphery_count)"
stale=$(infra_tag_line | grep -cv ":$MOVING_TAG\$" || true)

if [ "$stale" -gt 0 ]; then
  say "infra compose.yml does not name :$MOVING_TAG — bumping $INFRA_PATH"
  git -C "$INFRA_PATH" diff --quiet && git -C "$INFRA_PATH" diff --cached --quiet \
    || die "$INFRA_PATH has uncommitted changes; a bump would ride along with them"

  # Both lines move together: shipping one leaves the other naming a tag that
  # may exist in neither registry.
  # `#` as the delimiter, not `|`: the alternation below needs the pipe, and
  # escaping it as \| makes ERE read a LITERAL pipe, so the pattern silently
  # matches nothing.
  sed -i -E "s#^(\s*)image:\s*(\S*)/(core|periphery):.*#\1image: \2/\3:$MOVING_TAG#" \
    "$INFRA_PATH/compose.yml"
  # Keep the comment's version references honest too, or they rot into a lie
  # about which alias is floating.
  sed -i -E "s|\`[0-9]+\.[0-9]+\.[0-9]+-k8s\`|\`$MOVING_TAG\`|g; \
             s|\`[0-9]+\.[0-9]+\.[0-9]+-k8s-<sha>\`|\`$MOVING_TAG-<sha>\`|g" \
    "$INFRA_PATH/compose.yml"

  remaining=$(infra_tag_line | grep -cv ":$MOVING_TAG\$" || true)
  [ "$remaining" -eq 0 ] || die "rewrote compose.yml but $remaining image line(s) still do not name :$MOVING_TAG"

  git -C "$INFRA_PATH" add compose.yml
  git -C "$INFRA_PATH" commit -q -m "chore(komodo): move Core and Periphery onto the $MOVING_TAG images

Automated by the komodo fork's \`make deploy\` on a version change. Both
image lines move together; only \`core\` is deployed from Komodo."
  git -C "$INFRA_PATH" push -q origin HEAD
  # A push says the command ran. Compare refs before trusting it, because the
  # Stack deploys from the REMOTE, not from this checkout.
  local_sha=$(git -C "$INFRA_PATH" rev-parse HEAD)
  branch=$(git -C "$INFRA_PATH" rev-parse --abbrev-ref HEAD)
  remote_sha=$(git -C "$INFRA_PATH" ls-remote origin -h "refs/heads/$branch" | cut -f1)
  [ "$local_sha" = "$remote_sha" ] \
    || die "pushed $INFRA_PATH but remote $branch is $remote_sha, not $local_sha"
  say "infra bumped and pushed: ${local_sha:0:9}"
else
  say "infra compose.yml already names :$MOVING_TAG — nothing to bump"
fi

# --- 2. what SHOULD be running after this --------------------------------------
# The image id is the config-blob digest: identical on every host for the same
# image, so the build host's id is a valid expectation for the deploy host.
# The TAG proves nothing here - it is a floating alias that both the old and
# the new bytes have answered to.
expected_id=$(ssh -o BatchMode=yes "$BUILD_HOST" \
  "docker image inspect $HARBOR_REPO/core:$TAG --format '{{.Id}}'") \
  || die "could not read the image id from $BUILD_HOST; was the build pushed?"
say "expecting core image $expected_id"

server=$(api /read/GetStack "{\"stack\":\"$STACK\"}" | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["config"]["server_id"])')
before_id=$(api /read/InspectContainer \
  "{\"server\":\"$server\",\"container\":\"komodo-core-1\"}" | python3 -c \
  'import json,sys; print(json.load(sys.stdin).get("Image",""))' 2>/dev/null || echo "")
say "currently running $before_id"

# --- 3. deploy ------------------------------------------------------------------
say "deploying $STACK service core"
# Tolerate a dropped connection: Core may tear its own container down while
# this request is still open, and the poll below is the real verification
# anyway. A refusal that happens BEFORE the deploy starts still shows up as a
# timeout there rather than passing silently.
api /execute/DeployStack "{\"stack\":\"$STACK\",\"services\":[\"core\"]}" >/dev/null 2>&1 || \
  say "deploy request did not return cleanly (expected if Core shut down mid-request)"

# Core recreates its own container, so the Update it was writing is abandoned
# mid-flight and ALWAYS records success:false / "Komodo shutdown during
# execution". That record is not a verdict. Poll the running state instead.
say "waiting for Core to come back (its own Update will say success:false — expected)"
deadline=$(( $(date +%s) + 300 ))
while :; do
  got=$(api /read/InspectContainer \
    "{\"server\":\"$server\",\"container\":\"komodo-core-1\"}" 2>/dev/null | python3 -c \
    'import json,sys; print(json.load(sys.stdin).get("Image",""))' 2>/dev/null || echo "")
  [ "$got" = "$expected_id" ] && break
  [ "$(date +%s)" -lt "$deadline" ] || die "timed out; core is running '${got:-unreachable}', expected $expected_id"
  sleep 5
done

# --- 4. read back ---------------------------------------------------------------
running_version=$(api /read/GetVersion '{}' | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["version"])')
[ "$running_version" = "$VERSION" ] \
  || die "core image matches but it reports version $running_version, expected $VERSION"

say "core is on $expected_id and reports $running_version"
[ "$before_id" = "$expected_id" ] \
  && say "NOTE: the image id did not change — the build produced identical bytes"
say "agents are a separate rollout: cd $INFRA_PATH && ./upgrade-periphery.sh $VERSION"
