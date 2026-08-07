# Terraform Deployments for Kubernetes — Implementation Plan

*Status: DRAFT for review — no implementation started. Drafted 2026-08-04 on branch
`feature/kubernetes-support`. Beads: epic `planning-z4y` (children `planning-z4y.1`–`.8`).
Method: 4 code-mapping agents (this repo + aws-staging) + 5 definition-of-done panelists;
all claims below carry file:line references verified on this branch.*

## 1. Goal and interpretation

Add **Terraform as a deployment mechanism in Komodo**, so the Terraform units the team
already writes (aws-staging `terraform/live/*`) can be planned, applied, destroyed, and
audited from Komodo — with Kubernetes as the primary target.

The aws-staging spike has exactly two Terraform shapes, and this plan covers both:

| Shape | aws-staging example | What Komodo adds |
|---|---|---|
| **Deploy INTO a k8s cluster** via `kubernetes` ~2.38 + `helm` ~3.0 providers | `live/local/workloads`, `live/aws/workloads` (ingress-nginx, kube-prometheus-stack as `helm_release`) | Run/plan/audit from Komodo; kubeconfig sourced from the existing Cluster resource |
| **Provision cluster infra** | `live/aws/eks` (aws provider; kubeadm on-prem is shell, not tf) | Same engine, different unit; outputs → Cluster registration is a later follow-up |

**Assumptions (state once, then loop):**

- "Terraform deployment for kubernetes" = Komodo *runs* Terraform. It does **not** mean
  deploying Komodo itself onto k8s via Terraform (prod Core stays a docker deploy on
  172.21.10.106), and it does **not** mean building a Helm/Kustomize/Jsonnet rendering
  pipeline into the Cluster resource — `docs/argocd-komodo-parity.md:453` explicitly
  recommends against standing up a templating engine (tier-2 sprawl).
- A **generic** Terraform resource is the right unit: "deploy to k8s via terraform" is just
  a working dir whose providers happen to be kubernetes/helm. Komodo needs zero
  k8s-specific terraform logic beyond kubeconfig delivery (§3.7).
- Terraform stays a **binary shellout on a periphery host** — the same grain as the Cluster
  resource (kubectl) and every docker mutation (68/97 command sites are CLI shellouts).
  No SDK, no state parsing beyond exit codes, no typed HCL mirror.

If the intended meaning was narrower (e.g. only cluster provisioning, or only the
workloads shape), the design is unchanged — only the Phase 5 example flips.

### Why this is strategically cheap

- The `helm_release` provider fills the "Komodo has no Helm subsystem at all" gap
  (`docs/argocd-komodo-parity.md:264-290`, ~45 `missing` rows) without building one.
- Scheduled `terraform plan -detailed-exitcode` gives infra drift detection — something
  even Argo CD lacks (parity doc row 418).
- The whole engine reuses seams that already exist: manifest-source materialization,
  `[[VAR]]` interpolation + secret replacers, `run_komodo_command_with_sanitization`,
  process-group timeout kill, Update/audit plumbing, toml sync, e2e harness.

## 2. Alternatives considered (and why not)

| Option | Verdict |
|---|---|
| **Tier-0 only** — `Repo.on_pull` / `Stack.pre_deploy` `SystemCommand` runs terraform today (`bin/periphery/src/api/compose.rs:499-507`; there is *no* generic "run command on server" Execution variant) | Kept as Phase 0 validation spike, not the product: no permissions, no plan/apply separation, no audit semantics, secrets in shell strings |
| **Extend Cluster** with a terraform deploy mode | Wrong shape — terraform units aren't cluster-scoped (eks unit has no cluster yet; state/backends/providers are unit-level concerns). Cluster stays kubectl-only |
| **Action (Deno on Core)** | Actions execute on Core, not periphery (`bin/core/src/api/execute/action.rs:194-216`); Core hosts shouldn't hold provider creds or egress |
| **New top-level `Terraform` resource** | **Chosen** — the seams are all known and measured (§4, §6) |

Naming: `Terraform` (matches what it runs). The binary is image-baked and
version-pinned; if the team later switches to OpenTofu (provider zips already come from
OpenTofu's GitHub re-releases), the lift stanza changes and the resource name is a
cosmetic decision — flagged in §8 Q4.

## 3. Design

### 3.1 Entity — `client/core/rs/src/entities/terraform.rs`

Modeled on `ClusterConfig` (`cluster.rs:95-309`). Sketch:

```rust
TerraformConfig {
  server_id: String,            // periphery host that runs terraform
  // -- working dir source, precedence: FilesOnHost > LinkedRepo > Repo > Contents
  //    (same resolution shape as Cluster::manifest_source(), cluster.rs:388-398)
  files_on_host: bool,
  run_directory: String,        // unit dir within the source, e.g. terraform/live/local/workloads
  linked_repo: String,          // Komodo Repo resource id
  git_provider / git_account / repo / branch / commit / clone_path / reclone,  // inline repo (Cluster pattern)
  file_contents: String,        // inline .tf for small configs
  // -- run inputs
  environment: String,          // KEY=VALUE lines: TF_VAR_*, provider creds; [[VAR]] interpolated
  skip_secret_interp: bool,
  extra_args: Vec<String>,      // appended to plan/apply/destroy
  init_extra_args: Vec<String>, // e.g. -backend-config=...
  proxy_url: String,            // HTTPS_PROXY prefix for providers needing egress (cluster.rs:926-929 pattern)
  // -- state management (see 3.5)
  managed_local_state: bool,    // default true: init -backend-config=path=<root>/terraform/state/<name>.tfstate
  // -- k8s bridge (see 3.7)
  cluster_id: String,           // optional: materialize that Cluster's kubeconfig for this run
  // -- ops
  send_alerts: bool,            // drift alerts (Phase 6)
  webhook_enabled / webhook_secret,  // Phase 6+
  links: Vec<String>,
}
```

`TerraformActionState { initializing, planning, applying, destroying }` — **non-empty**,
modeled on `StackActionState` (`stack.rs:935`), *not* on `ClusterActionState` which is an
empty stub (a shipped gap — see §7). `busy()` gates concurrent runs.

List-item state derives from the last operation (`Ok / Drifted / Failed / Unknown`) —
**no polling loop**. Unlike Cluster's cheap reachability probe, "polling" terraform means
running a plan; drift checks are explicitly scheduled instead (Phase 6).

### 3.2 Execute API

`PlanTerraform` / `ApplyTerraform` / `DestroyTerraform` (+ batch variants), client types in
`client/core/rs/src/api/execute/terraform.rs`, resolvers in
`bin/core/src/api/execute/terraform.rs` mirroring `execute_manifests()`
(`bin/core/src/api/execute/cluster.rs:817-903`): permission gate → interpolate on Core
(one `Interpolator` pass so `secret_replacers` covers env + file_contents, the
`interpolated_cluster()` pattern at `bin/core/src/helpers/cluster.rs:33-67`) → resolve
source (Core flattens LinkedRepo so periphery never learns about Repo resources,
`helpers/cluster.rs:74-146`) → periphery request → merge logs/commit hash into Update.

Semantics:

- **Auto-init**: every plan/apply/destroy is preceded by `terraform init -input=false`
  (idempotent; `.terraform/` persists in the working dir so it's cheap after first run).
- **Plan = the diff verb**: `plan -detailed-exitcode` — exit 0 no changes, **2 = changes
  (success, state → Drifted)**, 1 = real error. Same shell wrapper technique as the
  `kubectl diff` exit-1 special case (`bin/periphery/src/api/cluster.rs:536-545`).
- **Apply/Destroy** run `-auto-approve -input=false`, reachable only through the gated
  `/execute` path. Permission levels: Plan = Execute, Apply = Execute, Destroy = Write
  (mirrors `ApplyClusterObject`'s reasoning: unpinned blast radius ⇒ higher level).
- Secret-bearing values never hit argv (`ps`-visible): they travel as env (`TF_VAR_*`)
  or a 0600 tfvars file — never `-var="k=v"`.

### 3.3 Periphery engine — `bin/periphery/src/api/terraform.rs`

Mirror of `api/cluster.rs` with these deliberate differences:

- **Working dirs are persistent and stable-named**:
  `<periphery_root>/terraform/<to_path_compatible_name(name)>/` — never `random_string`,
  never `temporary: true`, never `remove_dir_all`. Deleting a working dir that holds
  local backend state orphans real infrastructure. Repo sources clone under the standard
  `repo_dir()` (persists across runs; `reclone` is safe because state lives outside the
  checkout — see 3.5).
- **Every invocation sets an explicit timeout** (`CommandOptions{timeout, cancel}` — not
  `Default::default()`): a provider dialing a firewall-blocked endpoint hangs, and the
  process-group SIGKILL in `lib/command/src/lib.rs:210-213/299-308` only activates when
  timeout/cancel is set. (Cluster's own `apply()` currently passes `Default::default()`
  — a known gap, not a template; see §7.)
- **Output is size-bounded** before storage in the Update log — `lib/command/src/output.rs`
  has no truncation anywhere today, and a large plan can be megabytes.
- **Sanitization everywhere**: `run_komodo_command_with_sanitization` with
  `secret_replacers` on init/plan/apply/destroy *and on every error-path
  `Log::error(...)` push* (Cluster's `write_manifests` error path skips this — see §7).
- Fixed env per invocation: `TF_IN_AUTOMATION=true`, `-input=false`, `TF_CLI_CONFIG_FILE`
  pinned to the periphery-controlled mirror config (never inherited from a host user's
  `~/.terraformrc`), `HTTPS_PROXY` prefix only when `proxy_url` set.
- Binary resolved from the image-baked path only — no request field can name a binary.

### 3.4 Binary + providers behind the firewall

`registry.terraform.io` and `releases.hashicorp.com` are **blocked** at the proxy
(measured 2026-07-27, aws-staging `terraform/README.md:148-164`). Two-part answer:

1. **Binary** — multi-stage lift into `bin/periphery/aio.Dockerfile` from
   `docker.io/hashicorp/terraform:1.15.8` (Docker Hub passes the proxy; exact pin matches
   aws-staging `Makefile:42`), with a build-time smoke assert mirroring the kubectl one
   (`RUN terraform version | grep -q Terraform`). Same stanza style as the existing
   kubectl-from-kindest/node lift (`aio.Dockerfile:1-11,50-54`). aio-only — `make
   docker-push` builds only this Dockerfile; host-native amd64 (arm64 out of scope unless
   stated, §8 Q6).
2. **Provider plugins** — day 1: per-host **filesystem mirror** of OpenTofu-released
   provider zips under `${PERIPHERY_ROOT_DIRECTORY}/terraform/mirror/` (volume-mounted,
   not image-baked — providers churn faster than images and aws provider alone is huge),
   pointed at via `TF_CLI_CONFIG_FILE` exactly like aws-staging's working
   `provider_installation { filesystem_mirror ... }` block (`README.md:155-163`,
   `Makefile:632-645`). Durable: publish the zips once to the **Nexus raw repo
   (repo.vici.corp, in NO_PROXY)** and use `network_mirror` — aws-staging's CI already
   suggests this (`terraform/.gitlab-ci.yml:120-121`). Filed as `planning-z4y.8`.

Note the offline e2e path needs *neither*: the builtin `terraform_data` resource requires
zero provider downloads (§ Phase 6).

### 3.5 State backend policy

Everything in aws-staging is local-state today (S3 blocks commented out; no GitLab http
backend anywhere). Policy:

- **Default: managed periphery-local state.** `managed_local_state=true` injects
  `terraform init -backend-config="path=<periphery_root>/terraform/state/<name>.tfstate"`
  so state lives *outside* the git checkout (survives `reclone`), under the
  volume-mounted periphery root (survives container recreation — verified with a real
  `--force-recreate`, not a simulated flag). 0600 perms; state contains secrets in
  plaintext by design.
- **Opt-out for units that declare their own backend** (set `managed_local_state=false`;
  the unit's `backend "http"`/`backend "s3"` block wins). Upgrade path documented with a
  runnable example against **GitLab-managed state** (`gitlab.data.vici.corp` http backend
  — gives locking + central visibility) or MinIO S3. Team-level backend decision is §8 Q2
  — the resource works with any answer.
- Concurrency is two independent layers: Komodo action-state busy check (Core,
  Stack-pattern `api/execute/stack.rs:134-140`) + terraform's own state lock (never pass
  `-lock=false`).

### 3.6 UI — `ui/src/resources/terraform/`

Reference implementation: `ui/src/resources/cluster/` (config.tsx source tabs +
executions.tsx `ConfirmButton` pattern). Plan/apply/destroy responses are `Types.Update`,
so output renders through the existing generic update-log viewer — **no custom plan-diff
component** (Cluster's DiffCluster has none either). Registration must touch every
hand-maintained per-type map (silent-omission list, verified current paths):
`resources/index.ts`, `lib/hooks.ts`, `lib/socket.tsx` (full invalidation set),
`lib/color.ts`, `lib/icons.ts`, `components/permissions/specific-selector.tsx`,
`resources/alerter/config/resources.tsx`, `client/core/ts/src/responses.ts` + regenerated
`ui/public/client/*`. N/A by verification: `pages/dashboard/active.tsx` (busy-state map —
neither Cluster nor Swarm appear), `new-with-deploy-target.tsx` (Terraform is not a
Deployment/Stack deploy target).

### 3.7 Kubernetes bridge (`cluster_id`)

For workloads-shape units: Core resolves the referenced Cluster (validated at
create/update), interpolates its kubeconfig, periphery materializes it as a 0600 temp
file with **unconditional cleanup on every exit path** (exact `ClusterCommand::build` /
`cleanup` discipline, `api/cluster.rs:44-92,114-119,544-546`), and exports
`TF_VAR_kubeconfig_path` (the variable aws-staging units already consume,
`live/local/workloads/variables.tf:1-11`) plus `KUBE_CONFIG_PATH` for provider-default
setups. No typed k8s logic on the Komodo side.

## 4. Phases

Beads: `planning-z4y.1`–`.8`, dependencies wired so `bd ready` walks them in order.
Implementation follows the ≤5-files-per-batch discipline with verification between
batches; each phase ends with `cargo build` + `cargo fmt --check` + (ui phases)
`tsc --noEmit` green and a commit.

| # | Bead | Scope | Verify (phase gate) |
|---|---|---|---|
| 0 | `planning-z4y.1` | **Zero-code spike**: Komodo Repo resource on a periphery host, `on_pull` runs init/apply of `live/local/workloads` with mirror + kubeconfig + local state | The helm_release lands in the kubeadm cluster; facts recorded here (run duration, mirror friction, state location) |
| 1 | `planning-z4y.2` | Periphery engine: Dockerfile lift + wire types + terraform module (materialize, init/plan/apply/destroy, sanitize, exit codes, timeout, output cap) | Image builds behind proxy, smoke assert passes; module unit-exercised via periphery API |
| 2 | `planning-z4y.3` | Entities + DB (3 sites in `lib/database/src/lib.rs`) + `KomodoResource` impl + all exhaustive matches + the 5 manual-list sites + toml sync + `make gen-client` | `cargo check --workspace` (compiler enforces the exhaustive matches) + grep the 5 macro sites (NOT compiler-enforced) |
| 3 | `planning-z4y.4` | Execute APIs + Operation variants + permissions + action states | Two concurrent applies on one id: second rejected busy; Updates carry logs |
| 4 | `planning-z4y.5` | UI registration (all maps in §3.6) + config/executions pages | `yarn build` clean; every map greps positive |
| 5 | `planning-z4y.6` | `cluster_id` bridge + documented workloads example | Example unit deploys ingress-nginx into the kind/kubeadm cluster from the UI |
| 6 | `planning-z4y.7` | e2e (offline `terraform_data` fixture: CRUD/toml/permissions/full cycle/drift/failure/timeout/sanitization/skip-when-absent) + drift schedule → alert + docs | `scripts/e2e.sh` green incl. new tests; CI installs terraform and fails loudly if missing |
| — | `planning-z4y.8` | Infra (cross-repo): provider zips → Nexus raw repo `network_mirror` | `TF_CLI_CONFIG_FILE` pointing at repo.vici.corp resolves providers with no proxy env |

**Estimate.** Anchors: Swarm upstream = 125 days solo (+30k/−17k LOC); the Cluster
resource on this branch = ~1 week wall-clock at the current agent-assisted pace, and
Terraform is *thinner* than Cluster (no object browser, no pod logs/exec terminal, no
polling loop, no day-2 ops). Rough: Phase 0 = hours; Phases 1–4 ≈ 3–5 working days at
branch pace; Phases 5–6 ≈ 1–2 days. Silent caps: estimate excludes `planning-z4y.8`
(infra ticket, other-repo) and any GitLab/MinIO backend standing-up.

## 5. Definition of Done

Five independent lenses (per team workflow, the same five panelists verify the
implementation against these at the end; overlap between lenses is intentional
redundancy). Full checklists in §9 appendix — headline counts: core/backend 19 items,
periphery/security 20, UI 20, infra/ops 18, testing/release 19.

The four highest-risk items, pulled up for review attention:

1. **The 5 macro manual-list sites are grep-verified, not compile-verified**
   (`sync/file.rs:253`, `api/execute/sync.rs:159,240,289`, `api/write/sync.rs:805`) —
   missing one = sync deltas silently never applied.
2. **Secret hygiene end-to-end**: sanitization on all output paths *including error
   paths*; no secrets in argv; 0600 on state/tfvars/kubeconfig; e2e asserts a known
   secret literal absent from Update logs.
3. **State never lives in a deletable dir**: persistent stable-named working dirs,
   managed state path outside checkouts, survives container recreation (real
   `--force-recreate` check).
4. **Hang containment**: explicit timeout + process-group kill on every invocation, with
   an e2e that kills a deliberately-hung run and asserts no orphaned
   `terraform-provider-*` processes.

## 6. Corrections to prior docs discovered while planning

`docs/komodo-kubernetes-support-effort.md` predates this branch's Cluster implementation
and undercounts current reality (Cluster now ships: 994-LOC execute handler, 15 Operation
variants, full UI registration). Its "2 lib/database edit sites" is actually 3, and 4 of
its 8 UI map paths are a directory level off. This plan's numbers are re-verified on the
current branch; treat the effort doc's Tier-1 estimates as historical.

### 6.1 Phase 0 reconnaissance — measured 2026-08-07 (`planning-z4y.1`)

Surveyed before running anything. Several §3 assumptions need amending:

| § claim | Measured reality |
|---|---|
| §3.4 "binary lift from `docker.io/hashicorp/terraform:1.15.8`", pin at aws-staging `Makefile:42` | Pin is **correct** (`TF_IMAGE := hashicorp/terraform:1.15.8`, `Makefile:42`). But aws-staging never installs a terraform binary — it runs the **image as a container**, `--network host`, `-u $(UID):$(GID)`, mirror bind-mounted read-only at `/mirror` (`Makefile:58-73`). `terraform` is on **no** host in the fleet, nor on the workstation. A lifted binary is still viable, it is just not what exists today. |
| §3.4 mirror under `${PERIPHERY_ROOT_DIRECTORY}/terraform/mirror/`, cite `Makefile:632-645` | Mirror is `$(HOME)/tmp/tf-mirror` (`Makefile:26`), **223 MB**, holding `kubernetes 2.38.0`, `helm 3.2.0`, `tls 4.3.0`, `aws 6.56.0` zips plus a generated `terraformrc`. The `filesystem_mirror` heredoc is at **`Makefile:644`** (single line, not 632-645). |
| §3.7 "`TF_VAR_kubeconfig_path` — the variable aws-staging units already consume" | **Correct**, `live/local/workloads/variables.tf:1-11`. `TF_RUN_K8S` passes `TF_VAR_kubeconfig_path=/kube/config` from `KUBECONFIG_F ?= $(HOME)/.kube/poc.yaml`. |
| §3.5 "state lives outside the git checkout" | The unit declares `backend "local" { path = "terraform.tfstate" }` (`providers.tf`), i.e. **inside** the checkout today. Overriding via `-backend-config=path=` is the plan's job; note the unit hard-codes a relative path, so the override must be proven, not assumed. |

Candidate host **O3-prod-minio-10-136** (the `staging` Cluster's server, ssh alias `ROM_SMS`):
`kubelet` active and `/etc/kubernetes/manifests/` present, so it **is** the kubeadm control
plane. Has `docker`, `kubectl` v1.33.12, `helm` v3.19.0 (installed 2026-08-07,
`planning-2k0`), and `/etc/kubernetes/admin.conf` (0600 root). Has **no terraform** and
**no provider mirror** — both would have to be shipped (~223 MB) or the mirror published to
Nexus first (`planning-z4y.8`). Periphery there is a systemd binary running as root, so it
can both `docker run` and read `admin.conf`.

**Blocking safety finding:** `live/local/workloads` is **already applied** — ingress-nginx is
running in that cluster and the state lives on the workstation at
`terraform/live/local/workloads/terraform.tfstate` (26.5 KB). A Phase 0 run on another host
with fresh local state would try to **create resources that already exist**. So the spike
must be `init` + **`plan`** against a copy of the existing state, not `apply`: a clean
"No changes" proves mirror resolution, kubeconfig handling, network reachability and run
duration, while `apply` proves nothing further and risks a live cluster. Amend the Phase 0
row in §4 accordingly.

## 7. Pre-existing Cluster gaps surfaced by the DoD panel (tracked, not fixed in-band)

Filed as separate beads — none block this feature, but Terraform must not inherit them:

| Bead | Gap |
|---|---|
| `planning-fzc` (P2) | `write_manifests` error path pushes **unsanitized** logs (`bin/periphery/src/api/cluster.rs` error push ~L370) |
| `planning-izp` (P3) | `kubectl apply/delete/diff` run with **no timeout** (`Default::default()` CommandOptions, `cluster.rs` ~L548) |
| `planning-w3f` (P2) | `ClusterActionState {}` is an **empty stub** — nothing rejects concurrent deploys of one Cluster (Stack pattern exists at `api/execute/stack.rs:134-140`) |
| `planning-9xh` (P3) | Verify: `socket.tsx` Cluster ws-invalidation list parity vs Stack/Swarm; docsite `sync-resources.md` has no `[[cluster]]` example; `roadmap.md` Cluster line unmarked |

## 8. Open decisions for review

1. **Scope confirmation** — is "terraform deployment for kubernetes" primarily the
   *workloads* shape (deploy into clusters; Phase 5 example), the *provisioning* shape,
   or both? Design covers both; the example and docs emphasis follow your answer.
2. **State backend team standard** — accept "managed periphery-local" as the default with
   GitLab http backend as the documented upgrade, or stand up GitLab/MinIO state first
   and make remote the default? (aws-staging has no GitLab remote yet, which weakens the
   GitLab-backend option short-term.)
3. **Destroy permission level** — Write (proposed) vs Execute.
4. **Terraform vs OpenTofu** — binary lift is `hashicorp/terraform:1.15.8` (BUSL;
   internal use fine, but this fork is GPL-3.0 — shellout keeps that clean). Provider
   zips already come from OpenTofu releases. Switch to the OpenTofu binary now, later, or
   never?
5. **Plan-before-apply enforcement** — v1 applies directly (plan is advisory). Enforcing
   "apply only a saved plan file" is a possible v2 hardening; worth it?
6. **arm64** — `make docker-push` builds host-native amd64 only. Any arm periphery hosts
   in scope?
7. **Resource name** — `Terraform` vs something neutral (`Infra`, `Tofu`) given Q4.

## 9. Appendix — full DoD checklists (5 lenses)

### 9.1 Core/backend (19)

1. - [ ] `Terraform` implements all 16 required `KomodoResource` methods — verify: `cargo check -p komodo_core` compiles `bin/core/src/resource/terraform.rs` with zero trait-impl errors (mirror `resource/cluster.rs`).
2. - [ ] `Terraform`/`TerraformListItem`/`TerraformConfig`/`TerraformInfo`/`TerraformActionState` in `client/core/rs/src/entities/terraform.rs` with `#[typeshare]` on every public type — verify: `rg -n "typeshare" client/core/rs/src/entities/terraform.rs` count matches public structs/enums.
3. - [ ] `TerraformActionState` has one bool per execute op (initializing/planning/applying/destroying) + `.busy()` wired from `resource::Terraform::busy`, modeled on `StackActionState` (`stack.rs:935`) not the empty `ClusterActionState` — verify: `rg -n "struct TerraformActionState" -A6` shows non-empty fields.
4. - [ ] `action_states()` registry (`bin/core/src/helpers/action_state.rs`) carries `CloneCache<String, Arc<ActionState<TerraformActionState>>>` — verify: `rg -n "Terraform" bin/core/src/helpers/action_state.rs`.
5. - [ ] Concurrent Plan/Apply/Destroy on one id rejected via busy check (Stack pattern) — verify: test fires two ApplyTerraform concurrently; second errors "busy", not queued.
6. - [ ] `ResourceTarget`/`ResourceTargetVariant` gain `Terraform` with id-extraction/`to_string`/API-path arms — verify: `cargo check -p komodo_client` (E0004 at every non-exhaustive site).
7. - [ ] Every exhaustive match compiles with the new variant — `bin/core/src/{api/write/resource.rs, api/write/permissions.rs, helpers/query.rs, permission.rs, sync/user_groups.rs ×3, api/read/toml.rs, sync/replace_ids.rs, alert/{mod,discord,slack}.rs}` — verify: `cargo check --workspace` zero errors.
8. - [ ] `Operation` enum gains `PlanTerraform/ApplyTerraform/DestroyTerraform/CreateTerraform/UpdateTerraform/RenameTerraform/DeleteTerraform`, each wired to an Update-producing handler — verify: `rg -n "Terraform" client/core/rs/src/entities/mod.rs` + matching arms in `bin/core/src/api/execute/terraform.rs`.
9. - [ ] The 5 macro manual-list sites updated (NOT compiler-enforced — grep, don't trust `cargo check`): `sync/file.rs:253` (`extend_filtered!`), `api/execute/sync.rs:159` (`resolve_id_to_name!`), `:240` (`get_deltas!`), `:289` (no-changes aggregate), `api/write/sync.rs:805` (`push_updates!`) — verify: `rg -n "Terraform"` hits each of the 5.
10. - [ ] `lib/database/src/lib.rs`: import + `pub terraforms: Collection<Terraform>` + `resource_collection(&db, "Terraform")` init — verify: 3 hits, mirroring Cluster's lines 11/62/101.
11. - [ ] `ToToml for Terraform` + `ResourceSyncTrait`/`ExecuteResourceSync` impls + `api/read/toml.rs` macro tables — verify: `rg -n "impl.*for Terraform" bin/core/src/sync/toml.rs bin/core/src/sync/resources.rs`.
12. - [ ] ResourceSync diffing surfaces Terraform config changes — verify: sync-diff test with a modified Terraform toml block yields non-empty diff, not `no_changes()==true`.
13. - [ ] Plan/Apply/Destroy Updates persist periphery stdout/stderr, queryable via `GetUpdate`/`ListUpdates` — verify: run PlanTerraform; `db.updates.find({operation:"PlanTerraform"})` has non-empty logs.
14. - [ ] Execute request/response structs with `#[typeshare]` incl. batch variants (`BatchDeployCluster` pattern) — verify: `rg -n "Terraform" client/core/rs/src/api/execute/`.
15. - [ ] `make gen-client` regenerates types with no manual edits; `cargo check` + `tsc --noEmit` clean after — verify: run both.
16. - [ ] `cluster_id` validated to reference an existing Cluster in `validate_create_config`/`validate_update_config` — verify: bogus `cluster_id` rejected at create/update, not stored.
17. - [ ] Source precedence mirrors `ClusterManifestSourceKind` resolution exactly — verify: unit test all 4 modes + conflicting-fields precedence winner.
18. - [ ] `[[VAR]]` interpolation reuses the shared `Interpolator` (`lib/interpolate`), not a bespoke regex — verify: `rg -n "interpolate" bin/core/src/helpers/terraform.rs`.
19. - [ ] `cargo check --workspace` + `cargo clippy --workspace` clean vs pre-feature baseline — verify: run both, diff warning counts.

### 9.2 Periphery execution + secret hygiene (20)

1. - [ ] `secret_replacers` scrubs command/stdout/stderr for init/plan/apply/destroy AND error-path `Log::error` pushes (don't inherit Cluster's unsanitized `write_manifests` error path) — verify: grep every command call site + error push site in `bin/periphery/src/api/terraform.rs`.
2. - [ ] Materialized kubeconfig 0600 via `set_private()` (`api/cluster.rs:114-119` pattern) — verify: mid-run `ls -l` shows `-rw-------`.
3. - [ ] tfstate/tfstate.backup/tfvars written 0600 — verify: no bare `fs::write` without a permission call.
4. - [ ] Kubeconfig temp removed on EVERY exit path (unconditional cleanup before `result?`, `cluster.rs:544-546` shape) — verify: cleanup not inside `if success`.
5. - [ ] Working dir NEVER `remove_dir_all`'d when local backend in use — verify: grep returns nothing for the resource's own dir; never `temporary: true`.
6. - [ ] Working dir keyed by `to_path_compatible_name(name)`, not `random_string` — verify: grep; random naming would orphan state between runs.
7. - [ ] Working dir resolves under `periphery_config().root_directory` only; no path traversal from Core-supplied config — verify: resource named `../../etc` cannot escape root.
8. - [ ] Every invocation sets `CommandOptions{timeout: Some, cancel: Some}` (never `Default::default()` — Cluster's `apply()` gap, don't copy) — verify: grep.
9. - [ ] Hung terraform + provider children killed via process-group SIGKILL — verify: plan against unreachable endpoint with short timeout; `pgrep -f terraform-provider` empty after.
10. - [ ] Combined output size-bounded before Update storage (`lib/command/src/output.rs` has NO truncation today — net-new) — verify: cap constant exists and is exercised.
11. - [ ] `-input=false` on every invocation — verify: grep matches every constructed command.
12. - [ ] `TF_IN_AUTOMATION=true` in child env — verify: grep command builder.
13. - [ ] `TF_CLI_CONFIG_FILE` explicitly pinned to a periphery-controlled path, never inherited — verify: env explicitly set.
14. - [ ] Binary from fixed image-baked path only; NO request field names a binary; Dockerfile lift + `RUN terraform version` assert — verify: grep wire types + Dockerfile.
15. - [ ] `plan -detailed-exitcode`: 2 = success-with-changes; only 1 fails — verify: shell wrapper adapted from `kubectl diff` handling (`cluster.rs:536-543`).
16. - [ ] No secret `-var=` literals on argv — secrets via 0600 var-file or `TF_VAR_*` env — verify: grep command building for `-var=` fed from secret sources.
17. - [ ] `-auto-approve` reachable only through the gated `Execution` enum path — verify: variants live alongside `DeployCluster`, no query type mutates.
18. - [ ] Two concurrency layers: Core action-state busy check (Stack pattern `api/execute/stack.rs:134-140`) + never `-lock=false` — verify: grep both.
19. - [ ] Read-only ops (`show`/`output`, if added) never pass `-auto-approve`, cannot mutate — verify: code inspection of match arms.
20. - [ ] `cluster_id` bridge reuses `ClusterCommand`'s exact write/0600/cleanup sequence — verify: no shortcut skipping `set_private`.

### 9.3 UI (20)

1. - [ ] `"Terraform"` in `RESOURCE_TARGETS` (`ui/src/resources/index.ts`).
2. - [ ] `ResourceComponents.Terraform = TerraformComponents` (typed `RequiredResourceComponents` — tsc enforces completeness).
3. - [ ] `useAllResources` entry in `ui/src/lib/hooks.ts` (`ListTerraforms`).
4. - [ ] `socket.tsx` action-state invalidation: `GetTerraformActionState` per-type block.
5. - [ ] `socket.tsx` completed-update block covers FULL set: `ListTerraforms, ListFullTerraforms, GetTerraformsSummary, GetTerraform` minimum.
6. - [ ] `terraformStateIntention` in `lib/color.ts`.
7. - [ ] `Terraform:` icon in `lib/icons.ts`.
8. - [ ] `ALL_PERMISSIONS_BY_TYPE.Terraform` in `components/permissions/specific-selector.tsx`.
9. - [ ] Alerter picker block in `resources/alerter/config/resources.tsx`.
10. - [ ] `pages/dashboard/active.tsx` — confirmed N/A (no Cluster/Swarm entries there; only add if Terraform introduces busy-state listing).
11. - [ ] `client/core/ts/src/responses.ts` entries for all reads/writes/executes (≥ Cluster's count; executes map to `Types.Update`).
12. - [ ] `make gen-client` output committed: all three `ui/public/client/*` files contain Terraform types.
13. - [ ] Executions follow cluster/executions.tsx exactly: `ConfirmButton` + `useExecute` ×3, gated on missing `server_id`.
14. - [ ] Plan/Apply/Destroy output routes through the generic update-log viewer as `Types.Update` — no custom diff viewer.
15. - [ ] TOML sync UI shows Terraform automatically (generic; type-specific code NOT required — only fails if runtime omits it).
16. - [ ] `yarn build` clean.
17. - [ ] `npx tsc --noEmit` clean in `ui/`.
18. - [ ] Config page renders: server selector, 4-variant source selector, environment editor, extra_args, cluster link — visual check against `config.tsx`.
19. - [ ] Apply/Destroy behind confirm dialogs; nothing auto-executes on page load (no `useEffect` execute).
20. - [ ] `git diff --stat main...HEAD -- ui/src client/core/ts/src` matches exactly the enumerated map list — no map silently skipped.

### 9.4 Infra/ops (18)

1. - [ ] Multi-stage `FROM docker.io/hashicorp/terraform:${TF_VERSION} AS terraform` + `COPY --from=terraform` in `aio.Dockerfile`.
2. - [ ] `ARG TF_VERSION` pinned exact = `1.15.8` (aws-staging `Makefile:42`), not a range.
3. - [ ] Build-time smoke assert immediately after the COPY (mirror `aio.Dockerfile:54` kubectl assert).
4. - [ ] Full `docker build --no-cache` succeeds with zero connection attempts to registry.terraform.io / releases.hashicorp.com.
5. - [ ] The `FROM` pull rides daemon-level proxy config (not `--build-arg`) — same mechanism as the kindest/node pull.
6. - [ ] `make docker-push` (tls-intercepting proxy, `docker-ca`/`BUILD_ARGS` plumbing, `Makefile:193-205`) exits 0 with the terraform stage present.
7. - [ ] AIO image size delta measured and recorded (commit msg or bead).
8. - [ ] Lift scoped to `aio.Dockerfile` only (matches kubectl precedent; `multi-arch.Dockerfile` untouched).
9. - [ ] Platform scope is a stated decision — `docker-push` builds host-native (no `--platform`); arm64 explicitly in or out.
10. - [ ] Filesystem-mirror strategy documented with the working `provider_installation` block shape (aws-staging `README.md:155-164`).
11. - [ ] Mirror path under volume-mounted `${PERIPHERY_ROOT_DIRECTORY}` (compose/periphery.compose.yaml:47-52, dev.compose.yaml:29-33), not image-baked or /tmp.
12. - [ ] `TF_CLI_CONFIG_FILE` wired in periphery config/compose — verify: grep compose files + `config/periphery.config.toml`.
13. - [ ] Nexus raw-repo `network_mirror` documented as long-term (repo.vici.corp is in NO_PROXY; reachable proxy-less from a periphery host).
14. - [ ] Provider egress reuses the Cluster `proxy_url` command-prefix pattern (`cluster.rs:926-929`), no new mechanism.
15. - [ ] Default state path documented as `${PERIPHERY_ROOT_DIRECTORY}/terraform/...` with matching volume entries in both compose files.
16. - [ ] State survives `docker compose up -d --force-recreate periphery` — checksum before/after (real trigger, per coding-discipline §5).
17. - [ ] GitLab http / MinIO s3 backend upgrade documented with a runnable example block, not just named.
18. - [ ] `required_version >= 1.9` floor (all aws-staging units) satisfied by the 1.15.8 pin.

### 9.5 Testing / e2e / docs / release (19)

1. - [ ] `terraform_crud_round_trip` mirroring `e2e/tests/cluster.rs:38-128` — via `scripts/e2e.sh test`.
2. - [ ] `terraform_toml_sync_round_trip` (export → assert `[[terraform]]` → delete → recreate via RunSync → read-back match), mirroring `cluster.rs:135-228`.
3. - [ ] `terraform_hidden_from_unpermitted_user` using `non_admin_jwt` (`e2e/src/lib.rs:146,263`), mirroring `cluster.rs:230-282`.
4. - [ ] Fixture = inline `.tf` string (convention of `fn manifests()` in `cluster_deploy.rs`) using only `resource "terraform_data"`, no `provider` block.
5. - [ ] Full init→plan→apply→destroy cycle with zero provider downloads — no `.terraform/providers/` dir after.
6. - [ ] Drift test: mutate `terraform_data` trigger → re-plan → Update log signals changes (detailed-exitcode 2).
7. - [ ] Idempotency: second plan on unchanged fixture reports "No changes".
8. - [ ] Destroy leaves 0 resources in state — asserted on read-back.
9. - [ ] Failure path: failing fixture (`precondition`) → `await_update` errors / `Update.success == false` (not an HTTP error).
10. - [ ] Timeout: long `local-exec` sleep + short timeout → process group gone (`pgrep -f terraform` empty).
11. - [ ] Sanitization: configured Komodo secret literal absent from every Update log entry.
12. - [ ] `require_terraform!` skip macro mirroring `require_cluster!` (`e2e/src/lib.rs:172-197`); binary absent → "SKIP", suite still ok.
13. - [ ] CI installs terraform and FAILS (not skips) if missing — `.github/workflows/e2e.yml` install step + `TERRAFORM_AVAILABLE` gate mirroring `KIND_AVAILABLE` (`scripts/e2e.sh:217-227`).
14. - [ ] Production periphery image ships terraform (aio lift + `docker run --rm <img> terraform version`).
15. - [ ] `cargo build --verbose` + `cargo fmt --all -- --check` clean (ci.yml parity).
16. - [ ] `make gen-client` → `git status --porcelain client/core/ts ui/public/client` empty after commit.
17. - [ ] `docsite/docs/automate/sync-resources.md` gains a `[[terraform]]` example (note: `[[cluster]]` is missing there today — pre-existing gap, don't copy the omission).
18. - [ ] `roadmap.md` gains a Terraform line (and note Cluster's own line is still unmarked — see gaps bead).
19. - [ ] Full existing suite green: `scripts/e2e.sh` exit 0, all `cluster_*` tests unaffected.

## 10. Key references

- Cluster execute chain: `bin/core/src/api/execute/cluster.rs:817-903` → `bin/core/src/helpers/cluster.rs:33-146` → `bin/periphery/src/api/cluster.rs:319-607`
- Command primitives: `lib/command/src/lib.rs:190-308` (process-group kill), `run_komodo_command_with_sanitization:72`
- Interpolation: `lib/interpolate/src/lib.rs`, secrets assembly `bin/core/src/helpers/query.rs:429-451`
- kubectl lift precedent: `bin/periphery/aio.Dockerfile:1-11,50-54` (commit 35596f75b)
- aws-staging facts: `terraform/README.md:148-164` (blocked registries + mirror), `Makefile:42,632-645` (tf 1.15.8 pin, mirror build), `live/local/workloads/providers.tf:15-35` (kubeconfig-driven kubernetes+helm providers)
- Effort/parity docs: `docs/komodo-kubernetes-support-effort.md` (historical — see §6), `docs/argocd-komodo-parity.md:415-453`
