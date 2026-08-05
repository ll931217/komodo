---
type: project
title: 'Komodo — Terraform resource plan (drafted, pending review)'
as-of: '2026-08-04T00:00:00.000Z'
source: >-
  komodo repo, branch feature/kubernetes-support —
  docs/terraform-kubernetes-deployment-plan.md (source of truth; this page is a
  pointer/summary)
status: draft-for-review
ingested_via: 'mcp:put_page'
ingested_at: '2026-08-04T09:26:42.467Z'
source_kind: 'mcp:put_page'
---

# Komodo Terraform-for-Kubernetes deployment plan

Drafted 2026-08-04; Liang-Shih reviews 2026-08-05. Beads epic `planning-z4y` (.1–.8).

**Decision (proposed, not yet approved):** add a new top-level `Terraform` resource to the Komodo fork rather than extending Cluster or staying tier-0. Terraform runs as a binary shellout on periphery hosts — same grain as the Cluster resource's kubectl engine. Covers both aws-staging shapes: provisioning (live/aws/eks) and deploying into clusters via kubernetes+helm providers (live/*/workloads) — the latter fills Komodo's "no Helm subsystem" gap without building a rendering pipeline (argocd-parity doc advises against tier-2 templating engines).

**Key environment facts baked into the design:**
- registry.terraform.io + releases.hashicorp.com blocked at corporate proxy; terraform binary lifted from docker.io/hashicorp/terraform:1.15.8 (Docker Hub passes), provider zips via filesystem mirror → long-term Nexus repo.vici.corp network_mirror (bead planning-z4y.8).
- State: managed periphery-local by default, stored outside git checkouts via `init -backend-config=path=...` under the volume-mounted periphery root; GitLab http backend documented as upgrade path.
- Offline e2e uses builtin `terraform_data` resource — zero provider downloads.

**Open decisions for review:** scope emphasis (workloads vs provisioning), state backend standard, Destroy permission level, Terraform-vs-OpenTofu binary, plan-before-apply enforcement, arm64, resource name.

**Side finds:** DoD panel surfaced 4 pre-existing Cluster gaps (unsanitized error logs planning-fzc, no kubectl timeout planning-izp, empty ClusterActionState planning-w3f, registration polish planning-9xh) — filed as beads, not fixed in-band.

Related: [[projects/komodo]] (fork context), aws-staging terraform spike (terraform/README.md — evaluation, not adopted staging path).
