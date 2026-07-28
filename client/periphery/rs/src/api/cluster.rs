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
