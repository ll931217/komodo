use komodo_client::entities::update::Log;
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};

//

/// How to reach a Kubernetes cluster from this Periphery host.
///
/// Assembled by Core from the Cluster's config, with any
/// `[[VARIABLE]]` interpolation already applied, so Periphery never
/// needs to know about Komodo Variables.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ClusterTarget {
  /// Kubeconfig contents to write to a temporary file for the
  /// duration of the command. Takes precedence over `kubeconfig_path`.
  #[serde(default)]
  pub kubeconfig_contents: String,
  /// Path to an existing kubeconfig on this host.
  #[serde(default)]
  pub kubeconfig_path: String,
  /// Context to select, or empty for the kubeconfig's current context.
  #[serde(default)]
  pub context: String,
  /// Proxy used to reach the api server, passed as `HTTPS_PROXY`.
  #[serde(default)]
  pub proxy_url: String,
}

//

/// Check whether the Kubernetes api server is reachable
/// with the given credentials.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(PollClusterStatusResponse)]
#[error(anyhow::Error)]
pub struct PollClusterStatus {
  pub target: ClusterTarget,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PollClusterStatusResponse {
  /// Whether the api server answered.
  pub reachable: bool,
  /// The reported server version, when reachable.
  pub version: Option<String>,
  /// Why the probe failed, when unreachable.
  /// Already sanitized of any interpolated secrets.
  pub err: Option<String>,
}

//

/// Apply or delete Kubernetes manifests on a cluster.
///
/// Manifests arrive already interpolated; `secret_replacers` lets
/// Periphery scrub secret values out of the command output before it
/// is stored in the Update log.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<Log>)]
#[error(anyhow::Error)]
pub struct ApplyClusterManifests {
  pub target: ClusterTarget,
  /// The manifest contents to apply.
  pub manifests: String,
  /// Namespace passed to kubectl.
  pub namespace: String,
  /// Apply with kustomize (`-k`) rather than as plain resource files.
  #[serde(default)]
  pub kustomize: bool,
  /// What to do with the manifests.
  #[serde(default)]
  pub mode: ClusterApplyMode,
  /// Additional arguments passed to kubectl.
  #[serde(default)]
  pub extra_args: Vec<String>,
  /// (secret value, replacement) pairs scrubbed from the output.
  #[serde(default)]
  pub secret_replacers: Vec<(String, String)>,
}

/// What [ApplyClusterManifests] should do with the manifests.
#[derive(
  Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq,
)]
pub enum ClusterApplyMode {
  /// `kubectl apply`
  #[default]
  Apply,
  /// `kubectl delete`
  Delete,
  /// `kubectl diff` - reports pending changes, touches nothing.
  Diff,
}
