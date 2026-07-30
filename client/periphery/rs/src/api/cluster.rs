use komodo_client::entities::{RepoExecutionArgs, update::Log};
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
#[response(ApplyClusterManifestsResponse)]
#[error(anyhow::Error)]
pub struct ApplyClusterManifests {
  pub target: ClusterTarget,
  /// Where the manifests come from.
  pub source: ClusterManifestSource,
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

//

/// Read Kubernetes objects as opaque JSON.
///
/// Komodo does not model Kubernetes types, so the response is whatever
/// `kubectl get -o json` produced. `name` selects a single object;
/// without it the whole collection is returned.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(serde_json::Value)]
#[error(anyhow::Error)]
pub struct GetClusterResources {
  pub target: ClusterTarget,
  /// Kubernetes kind, as kubectl accepts it (`pods`, `deployments`).
  pub kind: String,
  /// Namespace to read from. Ignored for cluster-scoped kinds.
  #[serde(default)]
  pub namespace: String,
  /// A single object's name, or None for the whole collection.
  #[serde(default)]
  pub name: Option<String>,
  /// Read across every namespace instead of just `namespace`.
  #[serde(default)]
  pub all_namespaces: bool,
}

//

/// Delete a single Kubernetes object by name.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct DeleteClusterResource {
  pub target: ClusterTarget,
  pub kind: String,
  #[serde(default)]
  pub namespace: String,
  pub name: String,
}

//

/// Read a pod's logs.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct GetClusterPodLog {
  pub target: ClusterTarget,
  pub namespace: String,
  pub pod: String,
  /// Which container in the pod. Required only for multi-container
  /// pods; kubectl picks the sole container otherwise.
  #[serde(default)]
  pub container: Option<String>,
  /// How many lines from the end to return.
  #[serde(default = "default_tail")]
  pub tail: u64,
  /// Include logs from the previous, terminated instance of the
  /// container - the only way to see why a crashlooping pod died.
  #[serde(default)]
  pub previous: bool,
}

fn default_tail() -> u64 {
  100
}

/// Where Periphery should get a Cluster's manifests.
///
/// Core resolves a linked Komodo Repo into [ClusterManifestSource::Repo]
/// before sending, so Periphery never needs to know Repo resources
/// exist - the same split used for [ClusterTarget].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClusterManifestSource {
  /// Manifests managed in Komodo, already interpolated.
  Contents(String),
  /// Files already on this host.
  FilesOnHost {
    /// Directory the manifests live in.
    run_directory: String,
    /// Paths relative to `run_directory`. Empty applies the directory.
    file_paths: Vec<String>,
  },
  /// A git repo for Periphery to clone or pull.
  Repo {
    args: RepoExecutionArgs,
    /// Token from Core, when the repo is private.
    git_token: Option<String>,
    /// Delete and reclone rather than pull.
    reclone: bool,
    /// Directory within the repo holding the manifests.
    run_directory: String,
    /// Paths relative to `run_directory`. Empty applies the directory.
    file_paths: Vec<String>,
  },
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ApplyClusterManifestsResponse {
  pub logs: Vec<Log>,
  /// Set for repo sources, so a deploy records what it deployed.
  pub commit_hash: Option<String>,
  pub commit_message: Option<String>,
}
