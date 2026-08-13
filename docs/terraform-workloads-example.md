# Terraform resource: deploying INTO a Kubernetes cluster

Worked example of the *workloads* shape — a terraform unit whose providers are
`kubernetes` + `helm`, pointed at a cluster Komodo already knows about. It runs
the `terraform/live/local/workloads` unit from the staging-poc repo
(ingress-nginx + kube-prometheus-stack as `helm_release`) against the on-prem
kubeadm cluster.

The only Kubernetes-specific thing Komodo does here is deliver a kubeconfig:
`cluster_id` names a Komodo Cluster, Core interpolates its kubeconfig, and
Periphery materializes it as a 0600 temp file for the duration of the run,
exported as `TF_VAR_kubeconfig_path` and `KUBE_CONFIG_PATH`. Nothing else about
the resource is k8s-aware.

## The resource

```toml
[[terraform]]
name = "staging-workloads"
[terraform.config]
server = "O3-prod-minio-10-136"

# Where the tree comes from. The WHOLE tree is materialized, never just the
# unit: units reference ../../modules-style paths, and terraform refuses a
# module path that escapes the tree it was given.
linked_repo = "tf-spike-staging-poc"
run_directory = "terraform/live/local/workloads"

# The kubeconfig bridge. Read access to this Cluster is checked when the
# field is set, since attaching it hands the run cluster credentials.
cluster = "staging"

# Helm chart repos live outside the cluster and need the proxy; the api
# server must NOT go through it, or the helm provider gets a misleading
# `Forbidden`.
proxy_url = "http://172.21.10.22:8888/"
no_proxy = "172.21.0.0/16,.vici.corp,.vidi.com,localhost,127.0.0.1,10.0.0.0/8"

# Written to a private env file on the Server and sourced before the run —
# never onto the command line, where any process on the host could read it.
environment = """
TF_VAR_ingress_nginx_chart=/etc/komodo/repos/tf-spike-staging-poc/terraform/.charts/ingress-nginx-4.13.3.tgz
TF_VAR_grafana_admin_password=[[GRAFANA_ADMIN_PASSWORD]]
"""

# Default. Keeps the state file outside the checkout, under the Periphery
# root, so a reclone cannot orphan real infrastructure.
managed_state = true
```

Then, from the UI or the CLI:

```sh
km execute plan-terraform staging-workloads     # -detailed-exitcode: drift shows as state Drifted
km execute apply-terraform staging-workloads    # -auto-approve, Execute permission
km execute destroy-terraform staging-workloads  # -auto-approve, Write permission
```

## Prerequisites on the Server

The AIO Periphery image bakes all of this (`bin/periphery/aio.Dockerfile`).
A host running the systemd **binary** install has none of it and needs:

| What | Why |
|---|---|
| `terraform` 1.15.8 on `PATH` | Periphery shells out to it; the version matches aws-staging's pin |
| A provider filesystem mirror + `terraformrc` | `registry.terraform.io` is blocked at the proxy, so providers cannot be resolved online |
| `TF_CLI_CONFIG_FILE` in Periphery's environment | Without it terraform falls back to the blocked registry and hangs |

For a binary install, mirror the image's layout:

```sh
# terraform binary, from Docker Hub (which does pass the proxy)
cid=$(docker create hashicorp/terraform:1.15.8)
docker cp "$cid:/bin/terraform" /tmp/terraform && docker rm -f "$cid"
sudo install -m 0755 /tmp/terraform /usr/local/bin/terraform

# providers: the zips, laid out as registry.terraform.io/<namespace>/<type>/
sudo mkdir -p /usr/local/share/terraform/mirror
# ... copy terraform-provider-{kubernetes,helm}_*_linux_amd64.zip into place ...
sudo tee /usr/local/share/terraform/terraformrc >/dev/null <<'EOF'
provider_installation {
  filesystem_mirror {
    path    = "/usr/local/share/terraform/mirror"
    include = ["registry.terraform.io/*/*"]
  }
  direct { exclude = ["registry.terraform.io/*/*"] }
}
EOF

# Periphery must see it, so put it in the unit rather than a login shell
sudo mkdir -p /etc/systemd/system/periphery.service.d
sudo tee /etc/systemd/system/periphery.service.d/terraform.conf >/dev/null <<'EOF'
[Service]
Environment="TF_CLI_CONFIG_FILE=/usr/local/share/terraform/terraformrc"
EOF
sudo systemctl daemon-reload && sudo systemctl restart periphery
```

## Gotchas this example was built around

Each of these was an actual failure during the Phase 0 spike
(`docs/terraform-kubernetes-deployment-plan.md` §6.4), not a precaution.

- **Paths leak into state.** The helm provider stores the chart path string, so
  a chart referenced as `/charts/x.tgz` in one run and `/tf/.charts/x.tgz` in
  the next produces a spurious `1 to change`. Reference charts by their path
  inside the Periphery checkout and keep it stable.
- **Out-of-repo artifacts.** `terraform/.charts/*.tgz` is git-ignored, so a
  fresh clone will not have it: stage the tarball on the Server, or point
  `TF_VAR_ingress_nginx_chart` at a path that exists there.
- **One chart repo is proxy-blocked outright.** `kubernetes.github.io` returns
  `Forbidden` through the corporate proxy even with proxy env set — hence the
  local tarball. `raw.githubusercontent.com` (kube-prometheus-stack) passes.
- **`backend "local"` inside the unit is overridden, not honored.** The unit
  hard-codes `path = "terraform.tfstate"`; `managed_state` redirects it with
  `init -backend-config=path=` to `<periphery_root>/terraform/state/<name>.tfstate`.
  Set `managed_state = false` for a unit that declares its own remote backend.
- **Secrets never reach argv.** `environment` is written to a 0600 env file and
  sourced. Do not pass credentials as `-var=` in `extra_args`: the whole
  command line is visible to every process on the host.
