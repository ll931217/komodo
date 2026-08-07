use komodo_client::entities::{
  EnvironmentVar, RepoExecutionArgs, update::Log,
};
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};

//

/// Where Periphery should get a Terraform tree.
///
/// Core resolves a linked Komodo Repo into [TerraformSource::Repo]
/// before sending, so Periphery never needs to know Repo resources
/// exist - the same split used for Cluster manifests.
///
/// Whatever the source, the WHOLE tree is materialized, never just the
/// unit directory: units reference `../../modules`-style relative
/// paths, and terraform refuses to evaluate a module path that escapes
/// the tree it was given (measured in Phase 0, plan doc §6.4).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TerraformSource {
  /// Root config managed in Komodo, already interpolated. Written to
  /// the persistent working directory for this resource name.
  Contents(String),
  /// A tree already on this host.
  FilesOnHost {
    /// Directory holding the terraform tree.
    root_directory: String,
  },
  /// A git repo for Periphery to clone or pull.
  Repo {
    args: RepoExecutionArgs,
    /// Token from Core, when the repo is private.
    git_token: Option<String>,
    /// Delete and reclone rather than pull. Safe for state because
    /// managed state lives outside the checkout.
    reclone: bool,
  },
}

/// Which terraform verb to run after `init`.
#[derive(
  Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq,
)]
pub enum TerraformMode {
  /// `terraform plan -detailed-exitcode` - reports pending changes,
  /// touches nothing.
  #[default]
  Plan,
  /// `terraform apply -auto-approve`
  Apply,
  /// `terraform destroy -auto-approve`
  Destroy,
}

//

/// Run `terraform init` + one verb against a unit within a tree.
///
/// Config arrives already `[[VARIABLE]]`-interpolated from Core;
/// `secret_replacers` lets Periphery scrub secret values out of the
/// command output before it is stored in the Update log.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(RunTerraformResponse)]
#[error(anyhow::Error)]
pub struct RunTerraform {
  /// The Terraform resource name. Keys the persistent working
  /// directory (Contents sources) and the managed state file, so it
  /// must be stable across runs.
  pub name: String,
  /// Where the tree comes from.
  pub source: TerraformSource,
  /// Directory within the tree holding the unit to run
  /// (`-chdir`). Empty runs the tree root.
  #[serde(default)]
  pub run_directory: String,
  /// What to do with the unit.
  #[serde(default)]
  pub mode: TerraformMode,
  /// Redirect the local backend's state file to a periphery-managed
  /// path outside the checkout (survives reclone), via
  /// `init -backend-config=path=`. Off for units that declare their
  /// own remote backend.
  #[serde(default)]
  pub managed_state: bool,
  /// Env entries (`TF_VAR_*` and friends), already interpolated.
  /// Written to a private env file and sourced, never onto the
  /// command line.
  #[serde(default)]
  pub environment: Vec<EnvironmentVar>,
  /// Kubeconfig contents to materialize as a private temp file for
  /// the duration of the run, exported as `TF_VAR_kubeconfig_path`
  /// and `KUBE_CONFIG_PATH`. Takes precedence over `kubeconfig_path`.
  #[serde(default)]
  pub kubeconfig_contents: String,
  /// Path to an existing kubeconfig on this host, exported the same
  /// way.
  #[serde(default)]
  pub kubeconfig_path: String,
  /// Proxy for providers that fetch from outside the cluster (helm
  /// chart repos). Exported as HTTP_PROXY / HTTPS_PROXY. Load-bearing
  /// for any helm-using unit behind the corporate proxy (§6.4).
  #[serde(default)]
  pub proxy_url: String,
  /// NO_PROXY value exported alongside `proxy_url`, so the kubernetes
  /// api server is dialed directly rather than through the proxy.
  #[serde(default)]
  pub no_proxy: String,
  /// Additional arguments passed to the verb command.
  #[serde(default)]
  pub extra_args: Vec<String>,
  /// (secret value, replacement) pairs scrubbed from the output.
  #[serde(default)]
  pub secret_replacers: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct RunTerraformResponse {
  pub logs: Vec<Log>,
  /// Plan mode only: whether the plan found pending changes
  /// (`-detailed-exitcode`). None for apply / destroy.
  pub changes: Option<bool>,
  /// Set for repo sources, so a run records what it ran.
  pub commit_hash: Option<String>,
  pub commit_message: Option<String>,
}
