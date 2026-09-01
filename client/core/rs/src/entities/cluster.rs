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

  /// Kinds that may never be operated on through this Cluster.
  /// Matched case-insensitively against the singular kind, with
  /// wildcard support (`*role*`), or a regex wrapped in backslashes.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub exclude_kinds: Vec<String>,

  /// When non-empty, only these kinds may be operated on - allow-list
  /// mode. An inclusion also overrides an exclusion, so a broad
  /// exclude plus a narrow include is a usable pair.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub include_kinds: Vec<String>,

  /// Optional proxy used to reach the Kubernetes api server,
  /// passed to kubectl as `HTTPS_PROXY`.
  #[serde(default)]
  #[builder(default)]
  pub proxy_url: String,

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

  /// Whether `namespace` is permitted by the allow-list.
  /// An empty allow-list permits everything.
  pub fn namespace_allowed(&self, namespace: &str) -> bool {
    self.namespaces.is_empty()
      || self.namespaces.iter().any(|n| n == namespace)
  }
}

/// A Cluster's blast-radius controls, detached from the Cluster so
/// they can travel to Periphery in a request.
///
/// Core checks the manifests a user declared, which is the fast answer
/// and the one that produces a good error. It is not the whole answer:
/// declared text is not what reaches the cluster once helm renders or
/// kustomize rewrites `namespace:`, and for a repo- or host-sourced
/// Application Core has never seen the text at all. Periphery holds
/// the materialized objects, so the enforcing check runs there and
/// this is what it enforces against.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ManifestPolicy {
  /// Namespaces objects may land in. Empty permits every namespace.
  #[serde(default)]
  pub namespaces: Vec<String>,
  /// Whether cluster-scoped objects may be touched at all.
  #[serde(default = "default_cluster_resources")]
  pub cluster_resources: bool,
  /// Kinds that may never be operated on.
  #[serde(default)]
  pub exclude_kinds: Vec<String>,
  /// When non-empty, an allow-list that also overrides `exclude_kinds`.
  #[serde(default)]
  pub include_kinds: Vec<String>,
}

/// Hand-written rather than derived, because the derived bool default
/// is `false` - which for `cluster_resources` is the *strictest*
/// setting, not the absent one. A request that omits the policy would
/// then refuse every cluster-scoped object, which is not what "no
/// policy sent" means. It has to match ClusterConfig's own default.
impl Default for ManifestPolicy {
  fn default() -> Self {
    Self {
      namespaces: Vec::new(),
      cluster_resources: default_cluster_resources(),
      exclude_kinds: Vec::new(),
      include_kinds: Vec::new(),
    }
  }
}

impl From<&ClusterConfig> for ManifestPolicy {
  fn from(config: &ClusterConfig) -> Self {
    Self {
      namespaces: config.namespaces.clone(),
      cluster_resources: config.cluster_resources,
      exclude_kinds: config.exclude_kinds.clone(),
      include_kinds: config.include_kinds.clone(),
    }
  }
}

impl ManifestPolicy {
  /// Whether `namespace` is permitted. An empty allow-list permits
  /// everything.
  pub fn namespace_allowed(&self, namespace: &str) -> bool {
    self.namespaces.is_empty()
      || self.namespaces.iter().any(|n| n == namespace)
  }

  /// Err when the kind policy forbids operating on `kind`.
  ///
  /// `include_kinds` is an allow-list AND an override: a kind named
  /// there is permitted even if `exclude_kinds` would have caught it,
  /// which is what makes "exclude everything, include these" usable.
  pub fn check_kind(&self, kind: &str) -> anyhow::Result<()> {
    if kind_matches(&self.include_kinds, kind) {
      return Ok(());
    }
    if !self.include_kinds.is_empty() {
      anyhow::bail!(
        "Kind '{kind}' is not in this Cluster's included kinds {:?}",
        self.include_kinds
      );
    }
    if kind_matches(&self.exclude_kinds, kind) {
      anyhow::bail!(
        "Kind '{kind}' is excluded on this Cluster ({:?})",
        self.exclude_kinds
      );
    }
    Ok(())
  }

  /// Err when `kind` in `namespace` violates any of the three
  /// controls. `namespace` empty means the object did not name one,
  /// so it lands in whatever the command's `--namespace` said - which
  /// the caller has already checked.
  pub fn check_object(
    &self,
    kind: &str,
    namespace: &str,
  ) -> anyhow::Result<()> {
    self.check_kind(kind)?;
    if is_cluster_scoped_kind(kind) {
      if !self.cluster_resources {
        anyhow::bail!(
          "Kind '{kind}' is cluster-scoped, but this Cluster has cluster resources disabled"
        );
      }
      // A cluster-scoped object has no namespace to check, and
      // kubectl reports none for it.
      return Ok(());
    }
    if !namespace.is_empty() && !self.namespace_allowed(namespace) {
      anyhow::bail!(
        "Object '{kind}' targets namespace '{namespace}', which is not in this Cluster's allowed namespaces {:?}",
        self.namespaces
      );
    }
    Ok(())
  }
}

/// Every spelling of `kind` that kubectl would accept, lowercased.
///
/// kubectl takes `Secret`, `secret` and `secrets` for the same thing,
/// so a policy matching only one spelling is a policy nobody can rely
/// on. Stripping a trailing `s` is not enough: the plural of a kind
/// that already ends in `s` adds `es` (`Ingress` -> `ingresses`,
/// `StorageClass` -> `storageclasses`), so no single normalized form
/// exists. Every candidate is generated instead, and a pattern
/// matching any of them counts.
fn kind_forms(kind: &str) -> Vec<String> {
  let kind = kind.trim().to_lowercase();
  if kind.is_empty() {
    return Vec::new();
  }
  let mut forms = vec![kind.clone()];

  // The plural, in case the pattern was written that way.
  if kind.ends_with('s')
    || kind.ends_with('x')
    || kind.ends_with('z')
    || kind.ends_with("ch")
    || kind.ends_with("sh")
  {
    forms.push(format!("{kind}es"));
  } else if let Some(stem) = kind.strip_suffix('y') {
    forms.push(format!("{stem}ies"));
  } else {
    forms.push(format!("{kind}s"));
  }

  // The singular, in case the kind itself arrived plural.
  if let Some(stem) = kind.strip_suffix("ies") {
    forms.push(format!("{stem}y"));
  }
  if let Some(stem) = kind.strip_suffix("es") {
    forms.push(stem.to_string());
  }
  if let Some(stem) = kind.strip_suffix('s') {
    forms.push(stem.to_string());
  }

  forms.sort();
  forms.dedup();
  forms
}

fn kind_matches(patterns: &[String], kind: &str) -> bool {
  let forms = kind_forms(kind);
  patterns.iter().any(|pattern| {
    let pattern = pattern.trim().to_lowercase();
    match crate::matcher::Matcher::new(&pattern) {
      Ok(matcher) => forms.iter().any(|form| matcher.is_match(form)),
      Err(e) => {
        // A pattern that does not compile must not silently widen the
        // policy, but it also cannot be the thing that decides: it is
        // reported and skipped, and any valid sibling still applies.
        tracing::warn!("invalid kind pattern '{pattern}' | {e:#}");
        false
      }
    }
  })
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

/// Which execution, if any, is currently running against a Cluster.
///
/// Every field makes the Cluster busy, so executions on one Cluster are
/// serialized against each other rather than only against their own
/// kind: they share a kubeconfig and they act on the same live
/// objects. Deploying manifests is an Application concern and is
/// serialized on the Application, not here.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClusterActionState {
  pub applying_object: bool,
  pub deleting_object: bool,
  pub restarting_workload: bool,
  pub rolling_back_workload: bool,
  pub scaling_workload: bool,
  pub cordoning_node: bool,
  pub uncordoning_node: bool,
  pub draining_node: bool,
  pub rolling_back_helm_release: bool,
  pub uninstalling_helm_release: bool,
  pub creating_port_forward: bool,
  pub deleting_port_forward: bool,
}

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

/// One row of `kubectl api-resources` - a kind the cluster's api
/// server actually serves, including CRDs.
///
/// `namespaced` is the authoritative answer to a question
/// [CLUSTER_SCOPED_KINDS] can only guess at for built-in kinds.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClusterApiResource {
  /// The plural name kubectl accepts (`pods`, `deployments`).
  pub name: String,
  /// Short aliases (`po`, `deploy`).
  #[serde(default)]
  pub short_names: Vec<String>,
  /// Group and version (`apps/v1`, `v1`).
  pub api_version: String,
  /// Whether objects of this kind live in a namespace.
  pub namespaced: bool,
  /// Singular PascalCase kind (`Pod`, `Deployment`).
  pub kind: String,
  /// The verbs the api server allows (`get`, `list`, `watch`, ...).
  #[serde(default)]
  pub verbs: Vec<String>,
  /// Categories the kind belongs to (`all`).
  #[serde(default)]
  pub categories: Vec<String>,
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

  /// The derived bool default is `false`, which for cluster_resources
  /// is the strictest setting rather than the absent one. A request
  /// that omits the policy must not silently start refusing every
  /// cluster-scoped object.
  #[test]
  fn absent_policy_is_permissive_not_strict() {
    let policy = ManifestPolicy::default();
    assert!(policy.cluster_resources);
    assert!(policy.check_object("Namespace", "").is_ok());
    assert!(policy.check_object("Secret", "kube-system").is_ok());
  }

  #[test]
  fn policy_matches_the_cluster_config_it_came_from() {
    let config = ClusterConfig {
      namespaces: vec!["app".to_string()],
      cluster_resources: false,
      exclude_kinds: vec!["Secret".to_string()],
      ..Default::default()
    };
    let policy = ManifestPolicy::from(&config);

    assert!(policy.check_object("Deployment", "app").is_ok());
    // Wrong namespace.
    assert!(
      policy.check_object("Deployment", "kube-system").is_err()
    );
    // Excluded kind, in an allowed namespace.
    assert!(policy.check_object("Secret", "app").is_err());
    // Cluster-scoped while cluster resources are off.
    assert!(policy.check_object("ClusterRole", "").is_err());
  }

  /// An object that names no namespace lands in whatever the command's
  /// `--namespace` said, which the caller checked separately. Judging
  /// it here would reject every manifest that omits the field.
  #[test]
  fn object_without_a_namespace_is_left_to_the_command() {
    let policy = ManifestPolicy {
      namespaces: vec!["app".to_string()],
      ..Default::default()
    };
    assert!(policy.check_object("Deployment", "").is_ok());
  }

  /// A cluster-scoped kind is allowed through with no namespace check
  /// when cluster resources are on - kubectl reports no namespace for
  /// one, and an allow-list would otherwise reject it.
  #[test]
  fn cluster_scoped_kinds_skip_the_namespace_check() {
    let policy = ManifestPolicy {
      namespaces: vec!["app".to_string()],
      ..Default::default()
    };
    assert!(policy.check_object("ClusterRole", "").is_ok());
  }
}
