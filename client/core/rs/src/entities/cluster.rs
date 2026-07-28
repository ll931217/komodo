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

#[typeshare]
pub type ClusterQuery = ResourceQuery<ClusterQuerySpecifics>;

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
