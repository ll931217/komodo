# Application resource — Implementation Plan

*Status: DRAFT for review — no implementation started. Drafted 2026-08-14 on branch
`vici`. Decisions taken with the user up front: the resource is named **Application**,
the manifest fields **move off** Cluster rather than being duplicated, and an Application
targets **exactly one** Cluster.*

## 1. The problem, measured

`ListClusters` on the live instance returns eight Clusters. They are one cluster:

| Komodo Cluster | namespace | server | kubeconfig | source |
|---|---|---|---|---|
| staging-airflow | airflow | O3-prod-minio-10-136 | /etc/kubernetes/admin.conf | repo `staging`, `terraform/examples/airflow` |
| staging-ams | ams | *same* | *same* | `terraform/examples/ams` |
| staging-authentik | authentik | *same* | *same* | `terraform/examples/authentik` |
| staging-db | database | *same* | *same* | `terraform/examples/staging-db` |
| staging-harbor-pull | staging | *same* | *same* | `terraform/examples/harbor-pull` |
| staging-incident-data | incident-data | *same* | *same* | `terraform/examples/incident-data` |
| staging-pms | pms | *same* | *same* | `terraform/examples/pms` |
| staging-sms-sender | sms-sender | *same* | *same* | `terraform/examples/sms-sender` |

Every row carries a full copy of the same connection: same `server_id`, same
`kubeconfig_path`, same (empty) context. They differ only in `namespace` and
`run_directory`, both of which describe *a workload*, not *a cluster*.

The cause is structural, not user error: **a Cluster carries exactly one manifest
source**. `run_directory` + `file_paths` + `kustomize` are single-valued, so "deploy a
second thing to this cluster" has no expression other than "declare a second Cluster".
Consolidating to `staging` + `production` is impossible while the deploy unit and the
connection unit are the same resource.

Two symptoms worth recording, because they show the model is already leaking:

- `staging-ams` declares `namespace = "ams"`, while the kustomization it deploys sets
  `namespace: staging`. The Komodo field is decorative for kustomize sources — the
  manifests win.
- Eight copies of a cluster-admin credential path exist where one would do. Rotating the
  kubeconfig means editing eight resources.

## 2. Design

### 2.1 The split

**Cluster becomes connection + policy.** It answers "how do I reach this cluster, and
what is anyone allowed to do to it".

```
ClusterConfig (after)
  server_id, kubeconfig_contents, kubeconfig_path, skip_secret_interp,
  context, proxy_url,               // reach it
  namespace,                        // default namespace for cluster-level ops
  namespaces, cluster_resources,    // policy: blast-radius controls
  send_unreachable_alerts, links
```

It keeps every execution that is genuinely cluster-scoped administration, because those
target live objects rather than a declared desired state: `ApplyClusterObject`,
`DeleteClusterObject`, `RestartClusterWorkload`, `RollbackClusterWorkload`,
`ScaleClusterWorkload`, `CordonClusterNode`, `UncordonClusterNode`, `DrainClusterNode`,
`RollbackHelmRelease`, `UninstallHelmRelease`, `CreateClusterPortForward`,
`DeleteClusterPortForward`. It keeps the reachability probe and its state.

**Application is the deploy unit.** It answers "what do I deploy, where".

```
ApplicationConfig
  cluster_id,                       // exactly one, validated at create/update
  namespace,                        // must satisfy the Cluster's allow-list
  // source, same 4-mode precedence as Cluster/Terraform:
  //   files_on_host > linked_repo > repo > file_contents
  files_on_host, linked_repo,
  git_provider, git_https, git_account, repo, branch, commit, clone_path, reclone,
  run_directory, file_paths, kustomize, file_contents,
  skip_secret_interp,
  wait_ready, extra_args,
  send_alerts, webhook_enabled, webhook_secret, links
```

`ApplicationActionState { deploying, destroying, diffing }`, and
`ApplicationState { Deployed, Failed, Unknown }` derived from the last execution — the
same "no probe, the run reports" grain the Terraform resource established, for the same
reason: a Cluster's reachability is cheap to poll, an Application's *correctness* is not.

Executions: `DeployApplication`, `DestroyApplication`, `DiffApplication`, plus batch
variants. These are the current `DeployCluster` / `DestroyCluster` / `DiffCluster`
handlers with the manifest half moved across and the connection half read from the
referenced Cluster.

**Permissions compose rather than duplicate.** Deploying an Application requires Execute
on the Application *and* Read-attach on its Cluster (checked at create/update, the
`cluster_id` pattern the Terraform resource already uses). A user granted Execute on
`ams` cannot repoint it at the production Cluster without permission on that Cluster.

### 2.2 What this does NOT add

Deliberately out of scope, so the resource stays a deploy unit and not a platform:

- No templating engine (`docs/argocd-komodo-parity.md:453` recommends against it, and
  kustomize already covers the existing units).
- No multi-cluster targeting, no ApplicationSets, no generators. One Application, one
  Cluster — a second environment is a second Application, which is also where the
  namespace and values differ anyway.
- No sync waves / hooks / health assessment beyond the existing `wait_ready`
  (`kubectl rollout status`).
- No auto-sync loop. Drift for Applications is the same answer as for Terraform: a
  scheduled Procedure running `DiffApplication`, alerting on differences.

### 2.3 Alternatives rejected

| Option | Verdict |
|---|---|
| Aggregate kustomization: one Cluster per environment whose `run_directory` lists all apps as `resources:` | Works today with zero code, and was offered first. Rejected by the user: deploys become all-or-nothing and the blast radius of a routine change is the whole environment |
| Terraform resource per app (`cluster_id` bridge, shipped in `planning-z4y`) | Still the right answer for units that *are* terraform. For the eight kustomize dirs it means either baking the `kustomization` provider into the aio image or rewriting them as helm/kubernetes resources — a rewrite of the workloads to fit the tool |
| Keep Cluster-per-app, rename and tag | Honest and free, but the resource list stays a list of apps labelled "clusters", and the duplicated kubeconfig stays duplicated |
| Application targets many Clusters | Rejected with the user. Needs per-cluster overrides, per-cluster state and a partial-failure model — most of the complexity of ApplicationSets, for a fleet with one cluster today |

## 3. Migration

The eight existing Clusters must become one Cluster plus eight Applications, **without
touching anything running in Kubernetes**. This is safe because deleting a Komodo
resource never deletes cluster objects — the same property that made the accidental sync
prune recoverable.

1. Create Cluster `staging` (server `O3-prod-minio-10-136`, kubeconfig
   `/etc/kubernetes/admin.conf`), no manifest fields.
2. For each old Cluster, create an Application carrying its manifest half, `cluster_id`
   pointing at `staging`, name unchanged minus the `staging-` prefix (`ams`, `pms`, …).
3. `DiffApplication` each one. **Every diff must come back empty** — that is the proof
   the Application reproduces what the Cluster was deploying, and the gate before step 4.
4. Delete the eight old Clusters.

Steps 1–3 are additive and reversible; only step 4 is not, and by then step 3 has proven
the replacement. The script is a `write` + `execute` sequence against the API, and it
belongs in the repo rather than in a shell history.

Sync files referencing `[[cluster]]` blocks for the eight need rewriting to
`[[application]]` in the same change, or the next sync recreates them.

## 4. Phases

Each phase ends with `cargo check --workspace` + `cargo fmt --check` + (UI phases)
`tsc --noEmit` green and a commit, following the batch discipline the Terraform epic used.

| # | Scope | Verify |
|---|---|---|
| 1 | Entities + DB collection + `KomodoResource` impl + the 5 macro manual-list sites + `lib/database` 3 sites + toml sync + `make gen-client` | `cargo check --workspace` (compiler enforces exhaustive matches); grep the 5 macro sites, which it does not |
| 2 | Execute APIs: Deploy/Destroy/Diff + batch, action states, permission gates, Cluster lookup for connection | Two concurrent deploys on one Application: second rejected busy; Update carries logs |
| 3 | Strip the manifest half from Cluster: config fields, `DeployCluster`/`DestroyCluster`/`DiffCluster`, their action-state flags, webhook listener | `cargo check`; every removed field gone from `resources.json` |
| 4 | UI: `ui/src/resources/application/`, every per-type map (the list is enumerated in the Terraform epic's §3.6), Cluster config page loses the source tabs | `yarn build` + `tsc --noEmit` clean; each map greps positive |
| 5 | Migration script + docs (`sync-resources.md` `[[application]]` example, roadmap line) + run it against staging behind an empty-diff gate | All eight `DiffApplication` runs come back empty before any Cluster is deleted |
| 6 | e2e: CRUD, toml sync round-trip, permissions, deploy/diff/destroy against kind, namespace-policy enforcement inherited from the Cluster | `scripts/e2e.sh` green including new tests; existing `cluster_*` tests still green after the strip |

**Estimate.** The Terraform epic (comparable shape: new resource, execute APIs, UI
registration, e2e) took roughly a week of agent-assisted work. Application is *narrower*
in engine terms — no new binary, no provider mirror, the periphery side already exists —
but adds a **breaking change to a shipped resource plus a live migration**, which
Terraform did not have. Phases 1–2 ≈ 2 days, phase 3 ≈ 1 day, phase 4 ≈ 1–2 days,
phases 5–6 ≈ 1–2 days.

## 5. Definition of Done

1. `ListClusters` returns environments (`staging`), not applications.
2. One kubeconfig per real cluster. Rotating it is a one-resource edit.
3. `DeployApplication` on each of the eight reproduces exactly what the old Cluster
   deployed — proven by an empty `DiffApplication` before the old resources are deleted.
4. An Application cannot deploy outside its Cluster's `namespaces` allow-list, and cannot
   create cluster-scoped objects when the Cluster forbids them. The policy lives on the
   Cluster; the Application inherits it and cannot widen it.
5. Attaching an Application to a Cluster requires permission on that Cluster.
6. The 5 macro manual-list sites are grep-verified (they are not compile-verified, and a
   miss means sync deltas that are silently never applied).
7. `scripts/e2e.sh` green, including the existing `cluster_*` tests after the strip.
8. `docsite` sync example and roadmap updated; no `[[cluster]]` block in any sync file
   still carries manifest fields.

## 6. Answered (2026-08-14)

1. **Naming of the eight** — **drop the prefix**: `staging-ams` becomes `ams`. The
   environment is the Cluster now, so repeating it in the Application name is the same
   duplication this whole change removes. Sync files referencing the old names change in
   the same commit as the migration.
2. **Production cluster** — **later**. `O3-prod-minio-10-136` is the only control plane
   in the fleet today, so this epic delivers the model and one Cluster (`staging`); the
   second Cluster resource appears when the second cluster does. Nothing in the design
   waits on it.
3. **Webhooks** — **none wired today**, so nothing breaks when `webhook_enabled` moves.
   Wanted as a feature: an inbound webhook triggering `DeployApplication` on push, which
   is the automatic-deployment story. Tracked separately rather than smuggled into this
   epic — the listener wiring is its own change with its own auth surface.
