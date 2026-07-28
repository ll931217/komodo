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

## LIN-138 measured spike (2026-07-28)

A focused Core-only spike now proves the minimal vertical slice against a disposable kind v0.30.0 / Kubernetes v1.33.1 cluster: register an operator-approved server-local kubeconfig, list namespaces and pod/deployment/statefulset/daemonset workloads, stream bounded pod logs, execute an argv-only pod command, and idempotently set one fixed namespace annotation. The implementation is about 2.5k Rust LOC including 32 deterministic tests plus one ignored live-kind vertical-slice test, not a production `Cluster` resource.

### Current API and security boundary

- Core mounts six authenticated endpoints below `/kubernetes`. The production router composes the existing authentication middleware with the exact route-level gate exercised by tests: a missing authenticated user is rejected, disabled and non-admin users are forbidden, and enabled admin or super-admin users may proceed. This explicit spike gate is necessary because `ResourceTarget`, `ResourceTargetVariant`, `Permission`, and `KomodoResource` cannot represent a cluster today.
- Registration accepts only canonical regular files below the operator-configured `KOMODO_KUBECONFIG_ROOT` (default `/etc/komodo/kubeconfigs`). The root and every descendant directory must be root/Core-owned and not group/world writable; the kubeconfig must be Core-owned, no larger than 1 MiB, and inaccessible to group/other. Core parses the YAML and rejects `exec`, legacy `auth-provider`, and external certificate/key/token file references. It pins both metadata and a SHA-256 digest, reopens/revalidates/rehashes the file for every command, rewinds it, and passes that exact descriptor as `/proc/self/fd/3`; credential bytes and the source path never enter argv. Responses expose only `credential_source: "server_file_path"` and `credential_redacted: true`; restart loses registrations by design.
- Kubectl is resolved from the operator-controlled absolute `KOMODO_KUBECTL_PATH` (default `/usr/bin/kubectl`), never from request data or `PATH`. The canonical executable must be root/Core-owned, executable, and not group/world writable; its identity and SHA-256 digest are pinned, revalidated on every spawn, and the exact open descriptor is executed through `/proc/self/fd/4`. Invocations use discrete argv, null stdin, piped output, `kill_on_drop(true)`, explicit timeouts, an aggregate stdout-plus-stderr byte budget, and a process-wide eight-command semaphore. One deadline covers capacity wait, process completion, stdout, and stderr; every timeout/error/limit path terminates and reaps the child. A non-zero status is an error, never synthetic success.
- Every operation performs an operation-specific `kubectl auth can-i` preflight (`list`, pod-log `get`, pod-exec `create`, or namespace `get`/`patch`) and fails closed on denial or malformed output before invoking the operation. Kubernetes remains authoritative and may still deny the subsequent call if RBAC changes in the unavoidable preflight-to-use window.
- Log streaming uses an eight-item bounded data channel for backpressure plus a single-use terminal slot that cannot be displaced by saturated data. Its producer selects concurrently on output, the absolute deadline, and receiver closure, so even a quiet follow stream kills and reaps kubectl promptly after downstream disconnect and releases its semaphore permit before the client drains buffered data. The `application/x-ndjson` body has explicit `data`, `end`, and sanitized `error` events (`timeout`, `output_limit`, `read_failed`, or `process_failed`) after headers are committed. This is request/response streaming only; no Kubernetes watch is retained or reconnected.
- Namespace annotation accepts only `komodo.rs/lin-138-spike`, serializes the read/compare/write section process-wide, distinguishes an absent annotation from a present empty value, and skips mutation only when the exact requested value is already present. Registration accepts an identical replay but rejects a same-name registration with different path/context.

### Production seams requiring change

1. **Client entities and database:** define a persisted `Cluster` entity/config that stores a secret-file reference, never kubeconfig bytes; add database collection/index/migration and typeshare-generated client API types.
2. **Core resource dispatch:** implement `KomodoResource`, TOML sync, read/write/execute APIs, cache refresh, update/audit recording, dependency/deletion guards, and every exhaustive/manual `ResourceTargetVariant` / `ResourceTarget` dispatch site documented above.
3. **Permissions:** add cluster as a permission target and decide operation-specific levels (`read`, `logs`, `exec`, `mutate`). The spike's global-admin gate must not be the production authorization model. Kubernetes RBAC also remains authoritative; shared credentials mean Komodo users otherwise collapse to one Kubernetes identity.
4. **Execution placement:** move kubectl access behind Periphery or a dedicated in-cluster agent so Core does not require local cluster credentials and binaries. The spike now uses an operator-controlled absolute kubectl path, but production still needs capability/version discovery, immutable packaged binaries, process-group isolation, and credential rotation/revocation.
5. **API and UI:** move request/response types into `komodo_client`, include OpenAPI/client generation, audit-safe error mapping, UI resource registration, permission selectors, cache invalidation, logs/exec UX, and credential-path administration.
6. **Watch behavior:** production status needs list-then-watch with `resourceVersion`, bounded caches/queues, slow-consumer policy, cancellation, reconnect backoff/jitter, `410 Gone` relist, bookmarks, auth refresh, and observability. Never expose Secret payloads through watch events or errors.

### Extension versus core

- **Extension first (recommended):** a Periphery-backed action/procedure or narrow plugin keeps Kubernetes credentials near the target, avoids the 21+ Core resource dispatch seams, and can deliver apply/status/log workflows in roughly **2–4 engineer-weeks** plus hardening. It lacks native resource permissions, cache invalidation, typed UI, and first-class audit semantics.
- **Thin Core resource:** the spike reduces uncertainty around kubectl invocation but not integration breadth. Revise tier 1 from 6–10 to **8–12 engineer-weeks** for one experienced engineer: 2 weeks entities/persistence/permissions, 2 weeks Periphery transport and credential lifecycle, 2–3 weeks APIs/watch/log/exec safety, 1–2 weeks UI, and 1–3 weeks kind CI/security/release hardening.
- **Full core parity:** unchanged at **6–12 months solo** and still not recommended. Typed Kubernetes mirrors and controllers would duplicate mature Kubernetes products while retaining substantial long-term compatibility and security cost.

### Backlog recommendation

1. Ship an extension/Periphery prototype for read-only inventory and bounded logs; keep exec and mutation admin-disabled by default.
2. Decide credential identity and authorization: per-cluster service account, per-user impersonation plus access reviews, or an in-cluster agent. This blocks production schema design.
3. Add kind CI covering registration replay/conflict, namespace/workload parsing, log cancellation/backpressure/limits, exec timeout/non-zero status, annotation no-op/update, and credential redaction.
4. Only then introduce a persisted `Cluster` resource and Komodo permissions. Require explicit product demand before typed workload management or watch-backed UI caching.

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
