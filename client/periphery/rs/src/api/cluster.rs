use komodo_client::entities::{
  RepoExecutionArgs, SearchCombinator,
  application::HelmSource,
  cluster::{
    ClusterMetricsEntry, ClusterMetricsKind, ClusterPortForward,
    ManifestPolicy,
  },
  update::Log,
};
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
  /// Render with `helm template` before applying. Empty chart means
  /// no helm involvement.
  #[serde(default)]
  pub helm: HelmSource,
  /// Paths never applied, wildcard or backslash-wrapped regex.
  #[serde(default)]
  pub exclude_file_paths: Vec<String>,
  /// What to do with the manifests.
  #[serde(default)]
  pub mode: ClusterApplyMode,
  /// Additional arguments passed to kubectl.
  #[serde(default)]
  pub extra_args: Vec<String>,
  /// (secret value, replacement) pairs scrubbed from the output.
  #[serde(default)]
  pub secret_replacers: Vec<(String, String)>,
  /// After a successful apply, `kubectl rollout status` each applied
  /// workload and fail if they never become ready.
  #[serde(default)]
  pub wait_ready: bool,
  /// The Cluster's blast-radius controls, enforced here against the
  /// objects that actually reach the cluster rather than against the
  /// text a user declared. Default is "no policy", which permits
  /// everything namespaced.
  #[serde(default)]
  pub policy: ManifestPolicy,
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
  /// `kubectl get -l`. Core validates the charset before sending.
  #[serde(default)]
  pub label_selector: Option<String>,
  /// `kubectl get --field-selector`. Core validates the charset.
  #[serde(default)]
  pub field_selector: Option<String>,
  /// Truncate the collection to this many objects after the fact -
  /// kubectl has no true server-side limit for `get`.
  #[serde(default)]
  pub limit: Option<u32>,
  /// Project each object down to a compact summary row.
  #[serde(default)]
  pub summary: bool,
}

//

/// Run one command in a pod's container via non-interactive
/// `kubectl exec`, returning the combined output as a [Log].
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct ExecClusterPod {
  pub target: ClusterTarget,
  /// The pod's name. Core validates the charset before sending.
  pub pod: String,
  /// Which container in the pod, or None for the sole container.
  #[serde(default)]
  pub container: Option<String>,
  /// Namespace the pod lives in.
  #[serde(default)]
  pub namespace: String,
  /// The command. Periphery base64-wraps it so it reaches the
  /// container's `sh -c` without ever being parsed by the host shell.
  pub command: String,
}

//

/// `kubectl describe` for one object, as plain text.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(String)]
#[error(anyhow::Error)]
pub struct GetClusterDescribe {
  pub target: ClusterTarget,
  /// Kubernetes kind, as kubectl accepts it (`pods`, `deployments`).
  pub kind: String,
  /// The object's name. Core validates the charset before sending.
  pub name: String,
  /// Namespace the object lives in. Ignored for cluster-scoped kinds.
  #[serde(default)]
  pub namespace: String,
}

//

/// Get `kubectl top` node / pod usage rows.
/// Fails when the cluster has no metrics-server.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<ClusterMetricsEntry>)]
#[error(anyhow::Error)]
pub struct GetClusterTop {
  pub target: ClusterTarget,
  #[serde(default)]
  pub kind: ClusterMetricsKind,
  /// Namespace to read (pods only).
  #[serde(default)]
  pub namespace: String,
  /// Read across every namespace (pods only).
  #[serde(default)]
  pub all_namespaces: bool,
}

//

/// List helm releases on the cluster, as `helm list -o json` returns
/// them. Komodo does not model helm types.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(serde_json::Value)]
#[error(anyhow::Error)]
pub struct ListHelmReleases {
  pub target: ClusterTarget,
  /// Namespace to list from.
  #[serde(default)]
  pub namespace: String,
  /// List across every namespace instead of just `namespace`.
  #[serde(default)]
  pub all_namespaces: bool,
  /// (secret value, replacement) pairs scrubbed from the output,
  /// the same contract as [ApplyClusterManifests].
  #[serde(default)]
  pub secret_replacers: Vec<(String, String)>,
}

//

/// Get one helm release's revision history and user-supplied values,
/// as `helm history` / `helm get values` return them:
/// `{ "history": [...], "values": {...} }`.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(serde_json::Value)]
#[error(anyhow::Error)]
pub struct InspectHelmRelease {
  pub target: ClusterTarget,
  pub name: String,
  #[serde(default)]
  pub namespace: String,
  /// (secret value, replacement) pairs scrubbed from the output,
  /// the same contract as [ApplyClusterManifests].
  #[serde(default)]
  pub secret_replacers: Vec<(String, String)>,
}

//

/// `helm rollback`. Without a revision, helm rolls back to the
/// previous one.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct RollbackHelmRelease {
  pub target: ClusterTarget,
  pub name: String,
  #[serde(default)]
  pub namespace: String,
  #[serde(default)]
  pub revision: Option<u64>,
  /// (secret value, replacement) pairs scrubbed from the output,
  /// the same contract as [ApplyClusterManifests].
  #[serde(default)]
  pub secret_replacers: Vec<(String, String)>,
}

//

/// `helm uninstall`.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct UninstallHelmRelease {
  pub target: ClusterTarget,
  pub name: String,
  #[serde(default)]
  pub namespace: String,
  /// (secret value, replacement) pairs scrubbed from the output,
  /// the same contract as [ApplyClusterManifests].
  #[serde(default)]
  pub secret_replacers: Vec<(String, String)>,
}

//

/// Start a `kubectl port-forward` session on this Periphery host.
///
/// The session name arrives already scoped by Core
/// (`{cluster_id}:{name}`), so different Clusters on one host
/// cannot collide.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(ClusterPortForward)]
#[error(anyhow::Error)]
pub struct CreateClusterPortForward {
  pub target: ClusterTarget,
  /// Scoped session name.
  pub session: String,
  /// `pod/name` or `service/name`.
  pub resource: String,
  #[serde(default)]
  pub namespace: String,
  /// Port to bind on this host.
  pub local_port: u16,
  /// Port on the pod / service.
  pub remote_port: u16,
  /// Address to bind. Empty means 127.0.0.1.
  #[serde(default)]
  pub address: String,
}

//

/// List the port-forward sessions whose name starts with `prefix`,
/// reaping any whose kubectl has exited.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<ClusterPortForward>)]
#[error(anyhow::Error)]
pub struct ListClusterPortForwards {
  #[serde(default)]
  pub prefix: String,
}

//

/// Kill a port-forward session by scoped name.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct DeleteClusterPortForward {
  pub session: String,
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

/// The `kubectl rollout` verbs Periphery will run.
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum ClusterRolloutVerb {
  Restart,
  Undo,
}

/// `kubectl rollout restart|undo` on a workload.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct RolloutClusterWorkload {
  pub target: ClusterTarget,
  pub verb: ClusterRolloutVerb,
  pub kind: String,
  pub name: String,
  #[serde(default)]
  pub namespace: String,
}

//

/// `kubectl scale --replicas` on a workload.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct ScaleClusterResource {
  pub target: ClusterTarget,
  pub kind: String,
  pub name: String,
  pub replicas: u32,
  #[serde(default)]
  pub namespace: String,
}

//

/// `kubectl cordon|uncordon` a node.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct SetClusterNodeSchedulable {
  pub target: ClusterTarget,
  pub node: String,
  pub schedulable: bool,
}

//

/// `kubectl drain` a node.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct DrainClusterNode {
  pub target: ClusterTarget,
  pub node: String,
  #[serde(default)]
  pub force: bool,
  #[serde(default)]
  pub delete_emptydir_data: bool,
}

//

/// `kubectl apply -f` a single object's manifest, written to a private
/// temp file the way managed kubeconfigs are.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct ApplyClusterObject {
  pub target: ClusterTarget,
  pub contents: String,
  #[serde(default)]
  pub namespace: String,
}

//

/// Read a pod's logs.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct GetClusterPodLog {
  pub target: ClusterTarget,
  pub namespace: String,
  /// The pod's name. Core guarantees exactly one of `pod` /
  /// `label_selector` is set (an agent predating the selector fields
  /// fails loudly on a missing `pod`).
  #[serde(default)]
  pub pod: Option<String>,
  /// Logs of every matching pod (`-l`, with `--prefix`). Core
  /// validates the charset before sending.
  #[serde(default)]
  pub label_selector: Option<String>,
  /// Which container in the pod. Required only for multi-container
  /// pods; kubectl picks the sole container otherwise.
  #[serde(default)]
  pub container: Option<String>,
  /// `--all-containers`.
  #[serde(default)]
  pub all_containers: bool,
  /// kubectl `--since` duration. Core validates the charset.
  #[serde(default)]
  pub since: Option<String>,
  /// kubectl `--since-time` RFC3339 stamp. Core validates the charset.
  #[serde(default)]
  pub since_time: Option<String>,
  /// How many lines from the end to return.
  #[serde(default = "default_tail")]
  pub tail: u64,
  /// Include logs from the previous, terminated instance of the
  /// container - the only way to see why a crashlooping pod died.
  #[serde(default)]
  pub previous: bool,
  /// Enable `--timestamps`
  #[serde(default)]
  pub timestamps: bool,
}

fn default_tail() -> u64 {
  100
}

/// Search a pod log's tail using `grep`. All lines go to stdout.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct GetClusterPodLogSearch {
  pub target: ClusterTarget,
  pub namespace: String,
  pub pod: String,
  /// Which container in the pod. Required only for multi-container
  /// pods; kubectl picks the sole container otherwise.
  #[serde(default)]
  pub container: Option<String>,
  /// The terms to search for.
  pub terms: Vec<String>,
  #[serde(default)]
  pub combinator: SearchCombinator,
  #[serde(default)]
  pub invert: bool,
  /// Enable `--timestamps`
  #[serde(default)]
  pub timestamps: bool,
}

/// Where Periphery should get a Cluster's manifests.
///
/// Core resolves a linked Komodo Repo into [ClusterManifestSource::Repo]
/// before sending, so Periphery never needs to know Repo resources
/// exist - the same split used for [ClusterTarget].
///
/// The Repo variant is much larger than Contents, and that is left
/// alone deliberately: boxing it to equalize them would turn a struct
/// variant into a newtype variant, changing the JSON Core and
/// Periphery exchange. A rolling upgrade would then have a Core
/// sending a shape the older Periphery on a host cannot decode. One
/// of these is constructed per request and never held in a
/// collection, so the size difference costs nothing that a wire break
/// would be worth.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(clippy::large_enum_variant)]
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
  /// Diff mode only: whether the diff found differences between the
  /// manifests and the cluster. None for apply / delete.
  ///
  /// `kubectl diff` reports this by exiting 1, which Periphery maps
  /// back to success - without carrying it here, Core cannot tell a
  /// clean diff from a drifted one, and "drift detection" detects
  /// nothing.
  pub changes: Option<bool>,
  /// Set for repo sources, so a deploy records what it deployed.
  pub commit_hash: Option<String>,
  pub commit_message: Option<String>,
}
