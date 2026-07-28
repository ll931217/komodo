# Komodo Kubernetes Support — Effort Analysis

*Analyzed 2026-07-27 against komodo v2.2.0 (commit 5c301020c). Method: 4 parallel code-mapping agents over `bin/periphery`, `bin/core`, `ui/`, plus git-history measurement of the Swarm feature as a cost anchor.*

## Verdict

Roadmap lists it as **"Undecided: Support 'Cluster' resource — Manage Kubernetes cluster"** — unchanged since 2024-08-18, zero k8s commits/code/docs in 2,854 commits of history. Three implementation tiers exist; tier 0 costs hours, tier 2 costs 6–12 months solo.

| Tier | What you get | Cost |
|---|---|---|
| **0 — zero code, works today** | `Repo.on_clone`/`on_pull` + `Stack.pre_deploy`/`post_deploy` are arbitrary `SystemCommand` shellouts on a periphery host. Host with kubectl+helm+kubeconfig → `kubectl apply -k .` GitOps. Procedure schedules for drift re-apply, Server terminal for kubectl, audit trail via Updates. | hours |
| **1 — thin `Cluster` resource** | kubectl shellout backend, manifests via existing git/file materialization, `kubectl get -o json` lists rendered generically, pod logs, pod exec. Opaque JSON — no typed mirrors. | ~8–12k LOC, 6–10 weeks solo |
| **2 — full parity with Komodo's Docker feature set** | Typed k8s entities, Cluster as third deploy target for Deployment/Stack, watch-based status cache, alerts, toml sync, RBAC per object kind, UI parity with ~6.7k LOC of Docker/Swarm-shaped screens. | 30–50k LOC, 250–400 files, 6–12 months solo |

Recommendation: skip tier 2 — Argo CD / Flux / Rancher own that space; Komodo's edge is Docker hosts. Tier 1 fits the codebase grain surprisingly well (see "grain" below).

## Codebase measurements

- 102,032 LOC Rust + 56,514 LOC TS (ui). Zero `#[test]` anywhere; CI = `cargo build` + `cargo fmt` only.
- `bollard` confined to `bin/periphery` only. Reads = bollard (~17–19 endpoints); **every mutation = docker CLI shellout** (68 of 97 command sites build `docker ...` strings).
- `client/core/rs/src/entities/docker/` = 5,217 LOC of hand-transcribed Docker Engine API mirror types, used directly as wire types (no runtime abstraction/trait anywhere).
- `bin/periphery/src` = 11,996 LOC, ~66% docker-coupled. Runtime-agnostic already: transport/auth (Noise-protocol handshake over WSS, mogh_pki), git clone/pull, stack file materialization (`stack/write.rs`, 397 LOC), PTY terminal (docker appears as a format string on exactly 2 lines: `api/terminal.rs:127,181`), host stats (sysinfo).
- `bin/core/src` = 46,604 LOC. `KomodoResource` trait: 6 associated types + 16 mandatory methods (`resource/mod.rs:90-229`), plus `ToToml`/`ResourceSyncTrait` (1 required method `get_diff`) for sync.
- **Touch points per new resource type: 21 in core** — 11 compiler-enforced matches + **10 silent manual lists** (3 marked `// New resource types need to be added here manually.`). Missing `api/execute/sync.rs:344-397` = sync deltas computed but never applied, no error. Plus 2 in `lib/database`, 3 exhaustive alert matches (`alert/mod.rs`, `discord.rs`, `slack.rs`) if alerting wanted.
- Per-resource core slice (resource/ + read/ + write/ + execute/): Server 2,875 / Stack 3,492 / Deployment 2,576 / Swarm 1,439 LOC. Sum of all 11 = 18,586 LOC (40% of core).
- UI: registry = `ui/src/resources/index.ts` (`RESOURCE_TARGETS` + `ResourceComponents`, 17-field interface). Routing/sidebar/search/permissions UI generic. **8 hardcoded per-type maps silently omit a new resource**: `lib/hooks.ts` (useAllResources), `lib/socket.tsx` (ws cache invalidation — without it zero invalidation), `lib/color.ts`, `lib/icons.ts`, `permissions/specific-selector.tsx`, `alerter/config/resources.tsx`, `dashboard/active.tsx`, `new-with-deploy-target.tsx`.
- Docker-shaped UI needing k8s parallels: ~6,700 LOC (components/docker 704, pages/docker 1,138, containers.tsx 220, components/swarm 495, pages/swarm 1,647, stack-service 403, log-section, terminal pages — exec terminal hardcodes 4 target kinds in 5 places).
- typeshare type generation is automatic (`run gen-client`); everything consuming the types is manual.

## Swarm as cost anchor (v2.0 roadmap item — the *easy* analog)

- 46 commits, 183 unique files, **+30,169 / −17,163 lines**. Alive today: 9,464 LOC (ui 3,818 / core 2,029 / client types 1,753 / periphery 1,365 / periphery client 392 / docs 107).
- First dev commit 2025-11-19 → v2.0.0 GA 2026-03-24 = **125 days**; still being patched on unmerged `2.3.0` branch as of 2026-07-10.
- Swarm needed **zero new transport, zero new crates** — just more methods on the existing `bollard::Docker` connection. Kubernetes gets none of that reuse.
- Attaching Swarm as a *second* deploy target (`swarm_id` on Deployment/Stack) alone touched 40 files.

## Why k8s costs multiples of Swarm

1. **No client**: kube-rs (heavy dep tree) or kubectl shellout; plus kubeconfig/SA-token credential storage that doesn't exist today.
2. **Connection model mismatch**: everything routes Core → Periphery → one host. A cluster is not a host. Either pin the Cluster to a Server holding a kubeconfig (cheap, ugly) or build an in-cluster agent (expensive).
3. **Type surface**: k8s mirror types (pods/deploys/services/ingress/configmaps/secrets/PVCs/events) = 5–15k LOC new, or punt to opaque JSON.
4. **Domain mismatch**: `DeploymentConfig` is docker-run flags; `extra_args` splice raw into the `docker run` string. No replicas/probes/limits/selectors/PVCs.
5. **Identity**: a Cluster resource holds one credential → all Komodo users hit the cluster as the same k8s identity unless impersonation (`--as`) + SelfSubjectAccessReview is added. Decide early; security property, not a checkbox.
6. **No test safety net** for a feature of this size; k8s CI (kind/k3s) would have to be built from scratch.

## Codebase grain (favors tier 1)

Periphery is already a "write files + shell out + parse `--format json` + stream PTY" engine. kubectl drops into that mold: same command runner (`lib/command`), same file materialization, same terminal (2-line change), same registry-credential lookup.

## Upstream reality

Single dominant maintainer (mbecker20: 2,096 of 2,854 commits; next contributor 293). Releases squash-merge as mega-commits (v2.0.0 = 1,078 files, +99k/−63k) — an external tier-2 PR is effectively unmergeable; a fork means permanent rebase against squashes. GPL-3.0.
