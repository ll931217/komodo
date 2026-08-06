use bson::{Document, doc};
use derive_builder::Builder;
use derive_default_builder::DefaultBuilder;
use partial_derive2::Partial;
use serde::{Deserialize, Serialize};
use strum::Display;
use typeshare::typeshare;

use crate::{
  deserializers::{
    file_contents_deserializer, option_file_contents_deserializer,
    option_string_list_deserializer, string_list_deserializer,
  },
  entities::_Serror,
};

use super::resource::{Resource, ResourceListItem, ResourceQuery};

#[typeshare]
pub type ClusterListItem = ResourceListItem<ClusterListItemInfo>;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClusterListItemInfo {
  /// The Server holding the kubeconfig for this Cluster.
  pub server_id: String,
  /// The kubeconfig context in use.
  pub context: String,
  /// The default namespace for Cluster operations.
  pub namespace: String,
  /// The Cluster state
  pub state: ClusterState,
  /// If there is an error reaching the Cluster,
  /// the message will be given here.
  pub err: Option<_Serror>,
}

#[typeshare]
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  PartialOrd,
  Ord,
  Default,
  Serialize,
  Deserialize,
  Display,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum ClusterState {
  /// The Kubernetes api server responded to the reachability probe.
  Ok,
  /// The Kubernetes api server could not be reached
  /// using the configured kubeconfig / context.
  Unreachable,
  /// The Cluster has not been probed yet.
  #[default]
  Unknown,
}

#[cfg(feature = "utoipa")]
#[derive(utoipa::ToSchema)]
#[schema(as = Cluster)]
pub struct ClusterSchema(
  #[schema(inline)] pub Resource<ClusterConfig, ClusterInfo>,
);

#[typeshare]
pub type Cluster = Resource<ClusterConfig, ClusterInfo>;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClusterInfo {}

#[typeshare(serialized_as = "Partial<ClusterConfig>")]
pub type _PartialClusterConfig = PartialClusterConfig;

#[typeshare]
#[derive(
  Debug, Clone, Default, Serialize, Deserialize, Builder, Partial,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[partial_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(
  feature = "schemars",
  partial_derive(schemars::JsonSchema)
)]
#[diff_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[partial(skip_serializing_none, from, diff)]
pub struct ClusterConfig {
  /// The Server whose Periphery holds the kubeconfig
  /// and runs the kubectl commands for this Cluster.
  #[serde(default, alias = "server")]
  #[partial_attr(serde(alias = "server"))]
  #[cfg_attr(
    feature = "schemars",
    partial_attr(schemars(rename = "server"))
  )]
  #[builder(default)]
  pub server_id: String,

  /// Kubeconfig contents managed in Komodo, written to a file on the
  /// Server at execution time.
  ///
  /// Supports `[[VARIABLE]]` interpolation, so credentials can live in
  /// Komodo Variables / secrets instead of in this field. Any auth
  /// method kubectl understands is expressed here, including bearer
  /// token, client certificate, and `exec` credential plugins for
  /// EKS / GKE / AKS.
  ///
  /// Takes precedence over `kubeconfig_path`.
  #[serde(default, deserialize_with = "file_contents_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_file_contents_deserializer"
  ))]
  #[builder(default)]
  pub kubeconfig_contents: String,

  /// Path to an existing kubeconfig file on the Server.
  /// If both this and `kubeconfig_contents` are empty, Periphery uses
  /// the default kubectl resolution (`$KUBECONFIG`, then
  /// `~/.kube/config`).
  #[serde(default)]
  #[builder(default)]
  pub kubeconfig_path: String,

  /// Whether to interpolate Komodo Variables / secrets into
  /// `kubeconfig_contents`. Interpolated secret values are sanitized
  /// out of command output.
  #[serde(default)]
  #[builder(default)]
  pub skip_secret_interp: bool,

  /// The kubeconfig context to use.
  /// If empty, the kubeconfig's current context is used.
  #[serde(default)]
  #[builder(default)]
  pub context: String,

  /// The default namespace for Cluster operations.
  /// If empty, `default` is used.
  #[serde(default)]
  #[builder(default)]
  pub namespace: String,

  /// Restrict Cluster operations to these namespaces.
  /// Empty means every namespace is allowed.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub namespaces: Vec<String>,

  /// Whether cluster-scoped objects (Namespaces, ClusterRoles,
  /// CustomResourceDefinitions, ...) may be touched at all.
  /// Set false to limit this Cluster to namespaced objects.
  #[serde(default = "default_cluster_resources")]
  #[builder(default = "default_cluster_resources()")]
  #[partial_default(default_cluster_resources())]
  pub cluster_resources: bool,

  /// Optional proxy used to reach the Kubernetes api server,
  /// passed to kubectl as `HTTPS_PROXY`.
  #[serde(default)]
  #[builder(default)]
  pub proxy_url: String,

  /// Kubernetes manifests managed in Komodo, applied on Deploy.
  /// Supports `[[VARIABLE]]` interpolation.
  ///
  /// Used only when no other manifest source is configured. Precedence:
  /// `files_on_host`, then `linked_repo`, then `repo`, then this.
  #[serde(default, deserialize_with = "file_contents_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_file_contents_deserializer"
  ))]
  #[builder(default)]
  pub file_contents: String,

  /// Source the manifests from files already on the Server.
  /// Use `run_directory` and `file_paths` to point at them.
  #[serde(default)]
  #[builder(default)]
  pub files_on_host: bool,

  /// Choose a Komodo Repo (Resource) to source the manifests.
  #[serde(default)]
  #[builder(default)]
  pub linked_repo: String,

  /// The git provider domain. Default: github.com
  #[serde(default = "default_git_provider")]
  #[builder(default = "default_git_provider()")]
  #[partial_default(default_git_provider())]
  pub git_provider: String,

  /// Whether to use https to clone the repo (versus http).
  #[serde(default = "default_git_https")]
  #[builder(default = "default_git_https()")]
  #[partial_default(default_git_https())]
  pub git_https: bool,

  /// The git account used to access private repos.
  /// Empty string can only clone public repos.
  #[serde(default)]
  #[builder(default)]
  pub git_account: String,

  /// The repo to source manifests from: {namespace}/{repo_name}
  #[serde(default)]
  #[builder(default)]
  pub repo: String,

  /// The branch of the repo. Default: main
  #[serde(default = "default_branch")]
  #[builder(default = "default_branch()")]
  #[partial_default(default_branch())]
  pub branch: String,

  /// Optionally pin a specific commit hash.
  #[serde(default)]
  #[builder(default)]
  pub commit: String,

  /// Optionally set an alternate clone path on the Server.
  #[serde(default)]
  #[builder(default)]
  pub clone_path: String,

  /// Delete and reclone the repo instead of pulling it.
  #[serde(default)]
  #[builder(default)]
  pub reclone: bool,

  /// The directory the manifests live in, relative to the repo root or
  /// to the host filesystem root for `files_on_host`.
  #[serde(default)]
  #[builder(default)]
  pub run_directory: String,

  /// Manifest paths relative to `run_directory`.
  /// Empty applies the whole directory.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub file_paths: Vec<String>,

  /// Whether incoming webhooks trigger a Deploy for this Cluster.
  #[serde(default = "default_webhook_enabled")]
  #[builder(default = "default_webhook_enabled()")]
  #[partial_default(default_webhook_enabled())]
  pub webhook_enabled: bool,

  /// An alternate webhook secret for this Cluster.
  /// Empty uses the default secret from the core config.
  #[serde(default)]
  #[builder(default)]
  pub webhook_secret: String,

  /// Apply with kustomize (`kubectl apply -k`) instead of
  /// treating the manifests as plain resource files.
  #[serde(default)]
  #[builder(default)]
  pub kustomize: bool,

  /// After a successful apply, wait for the applied workloads to roll
  /// out (`kubectl rollout status`) and fail the Deploy if they never
  /// become ready. Without it a Deploy succeeds as soon as the api
  /// server accepts the manifests, even if every pod crashloops.
  #[serde(default)]
  #[builder(default)]
  pub wait_ready: bool,

  /// Additional arguments passed to `kubectl apply` / `kubectl delete`.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub extra_args: Vec<String>,

  /// Whether to alert when this Cluster becomes unreachable.
  #[serde(default = "default_send_unreachable_alerts")]
  #[builder(default = "default_send_unreachable_alerts()")]
  #[partial_default(default_send_unreachable_alerts())]
  pub send_unreachable_alerts: bool,

  /// Configure quick links that are displayed in the resource header
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub links: Vec<String>,
}

fn default_cluster_resources() -> bool {
  true
}

fn default_send_unreachable_alerts() -> bool {
  true
}

fn default_git_provider() -> String {
  String::from("github.com")
}

fn default_git_https() -> bool {
  true
}

fn default_branch() -> String {
  String::from("main")
}

fn default_webhook_enabled() -> bool {
  true
}

/// Kubernetes kinds that are cluster-scoped rather than namespaced.
///
/// Not exhaustive - CRDs can define either scope and are not known
/// ahead of time. Used to enforce [ClusterConfig::cluster_resources],
/// so the list only needs to cover the built-in kinds whose blast
/// radius reaches outside a namespace.
pub const CLUSTER_SCOPED_KINDS: &[&str] = &[
  "APIService",
  "CSIDriver",
  "CSINode",
  "ClusterRole",
  "ClusterRoleBinding",
  "CustomResourceDefinition",
  "IngressClass",
  "MutatingWebhookConfiguration",
  "Namespace",
  "Node",
  "PersistentVolume",
  "PriorityClass",
  "RuntimeClass",
  "StorageClass",
  "ValidatingWebhookConfiguration",
];

/// Whether `kind` is one of the known cluster-scoped kinds.
/// Case-insensitive, and tolerates the plural/short forms kubectl
/// accepts (`namespaces`, `ns`, `clusterroles`).
pub fn is_cluster_scoped_kind(kind: &str) -> bool {
  let kind = kind.trim().trim_end_matches('s').to_lowercase();
  if kind == "n" {
    // `ns` reduced to `n` by the plural trim.
    return true;
  }
  CLUSTER_SCOPED_KINDS
    .iter()
    .any(|known| known.to_lowercase().trim_end_matches('s') == kind)
}

impl ClusterConfig {
  /// The namespace a Cluster operation targets when none is given.
  pub fn default_namespace(&self) -> &str {
    if self.namespace.is_empty() {
      "default"
    } else {
      &self.namespace
    }
  }

  /// Which manifest source this Cluster uses.
  ///
  /// Only one applies, so the order is fixed rather than left to
  /// whichever field happens to be set: host files, then a linked
  /// Repo, then an inline repo, then contents managed here.
  pub fn manifest_source(&self) -> ClusterManifestSourceKind {
    if self.files_on_host {
      ClusterManifestSourceKind::FilesOnHost
    } else if !self.linked_repo.is_empty() {
      ClusterManifestSourceKind::LinkedRepo
    } else if !self.repo.is_empty() {
      ClusterManifestSourceKind::Repo
    } else {
      ClusterManifestSourceKind::Contents
    }
  }

  /// Whether `namespace` is permitted by the allow-list.
  /// An empty allow-list permits everything.
  pub fn namespace_allowed(&self, namespace: &str) -> bool {
    self.namespaces.is_empty()
      || self.namespaces.iter().any(|n| n == namespace)
  }
}

/// Where a Cluster's manifests come from.
#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum ClusterManifestSourceKind {
  /// Files already present on the Server.
  FilesOnHost,
  /// A Komodo Repo resource.
  LinkedRepo,
  /// A git repo configured on the Cluster itself.
  Repo,
  /// Manifests managed in Komodo.
  Contents,
}

impl From<&Cluster> for crate::entities::RepoExecutionArgs {
  fn from(cluster: &Cluster) -> Self {
    Self {
      name: cluster.name.clone(),
      provider: cluster.config.git_provider.clone(),
      https: cluster.config.git_https,
      account: crate::entities::optional_string(
        &cluster.config.git_account,
      ),
      repo: crate::entities::optional_string(&cluster.config.repo),
      branch: cluster.config.branch.clone(),
      commit: crate::entities::optional_string(
        &cluster.config.commit,
      ),
      destination: None,
      default_folder: crate::entities::DefaultRepoFolder::Stacks,
    }
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::PartialSchema for PartialClusterConfig {
  fn schema()
  -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    utoipa::schema!(#[inline] std::collections::HashMap<String, serde_json::Value>).into()
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::ToSchema for PartialClusterConfig {}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClusterActionState {}

/// What `kubectl top` should measure.
#[typeshare]
#[derive(
  Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum ClusterMetricsKind {
  #[default]
  Pods,
  Nodes,
}

/// One row of `kubectl top nodes` / `kubectl top pods`.
///
/// Values stay in kubectl's own units ("250m", "1957Mi", "12%"):
/// they are display strings, not numbers to aggregate.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClusterMetricsEntry {
  pub name: String,
  /// Empty for nodes.
  #[serde(default)]
  pub namespace: String,
  /// CPU usage, eg. "250m".
  pub cpu: String,
  /// CPU percent of allocatable, eg. "12%". Nodes only.
  #[serde(default)]
  pub cpu_percent: String,
  /// Memory usage, eg. "1957Mi".
  pub memory: String,
  /// Memory percent of allocatable, eg. "51%". Nodes only.
  #[serde(default)]
  pub memory_percent: String,
}

/// A `kubectl port-forward` session running on the Cluster's Server.
///
/// The listen address is on the Server (Periphery host), not the
/// browser: reach it from machines that can reach the Server.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClusterPortForward {
  /// User-given session name, unique per Cluster.
  pub name: String,
  /// What is forwarded to, eg. `pod/api-0` or `service/api`.
  pub resource: String,
  pub namespace: String,
  /// Port bound on the Server.
  pub local_port: u16,
  /// Port on the pod / service.
  pub remote_port: u16,
  /// Address bound on the Server. Default 127.0.0.1;
  /// 0.0.0.0 exposes the forward to the Server's network.
  pub address: String,
  /// Whether the kubectl process is still running.
  pub alive: bool,
}

#[typeshare]
pub type ClusterQuery = ResourceQuery<ClusterQuerySpecifics>;

#[typeshare]
#[derive(
  Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum ClusterSortBy {
  /// Sort by name. Default.
  #[default]
  Name,
  /// Sort by state.
  State,
}

#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, Default, DefaultBuilder,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClusterQuerySpecifics {
  /// Filter clusters by server ids.
  pub servers: Vec<String>,
}

impl super::resource::AddFilters for ClusterQuerySpecifics {
  fn add_filters(&self, filters: &mut Document) {
    if !self.servers.is_empty() {
      filters
        .insert("config.server_id", doc! { "$in": &self.servers });
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn recognises_cluster_scoped_kinds() {
    for kind in ["Namespace", "namespaces", "ns", "ClusterRole"] {
      assert!(
        is_cluster_scoped_kind(kind),
        "{kind} should be cluster-scoped"
      );
    }
    for kind in ["Pod", "pods", "Deployment", "ConfigMap", "secret"] {
      assert!(
        !is_cluster_scoped_kind(kind),
        "{kind} should be namespaced"
      );
    }
  }

  #[test]
  fn manifest_source_precedence() {
    let mut config = ClusterConfig::default();
    // Nothing set at all still resolves to something applyable.
    assert_eq!(
      config.manifest_source(),
      ClusterManifestSourceKind::Contents
    );

    config.repo = "org/manifests".to_string();
    assert_eq!(
      config.manifest_source(),
      ClusterManifestSourceKind::Repo
    );

    // A linked Repo wins over an inline repo.
    config.linked_repo = "my-repo".to_string();
    assert_eq!(
      config.manifest_source(),
      ClusterManifestSourceKind::LinkedRepo
    );

    // Host files win over everything.
    config.files_on_host = true;
    assert_eq!(
      config.manifest_source(),
      ClusterManifestSourceKind::FilesOnHost
    );
  }

  #[test]
  fn namespace_rules() {
    let mut config = ClusterConfig::default();
    // Empty namespace falls back to kubectl's own default.
    assert_eq!(config.default_namespace(), "default");
    config.namespace = "app".to_string();
    assert_eq!(config.default_namespace(), "app");

    // Empty allow-list permits everything.
    assert!(config.namespace_allowed("anything"));

    config.namespaces =
      vec!["app".to_string(), "app-staging".to_string()];
    assert!(config.namespace_allowed("app"));
    assert!(config.namespace_allowed("app-staging"));
    assert!(!config.namespace_allowed("kube-system"));
    // Prefixes must not slip through.
    assert!(!config.namespace_allowed("app-prod"));
  }
}
