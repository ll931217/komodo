use bson::{Document, doc};
use derive_builder::Builder;
use derive_default_builder::DefaultBuilder;
use partial_derive2::Partial;
use serde::{Deserialize, Serialize};
use strum::Display;
use typeshare::typeshare;

use crate::{
  deserializers::{
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

  /// Path to the kubeconfig file on the Server.
  /// If empty, Periphery uses the default kubectl resolution
  /// (`$KUBECONFIG`, then `~/.kube/config`).
  #[serde(default)]
  #[builder(default)]
  pub kubeconfig_path: String,

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

  /// Configure quick links that are displayed in the resource header
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub links: Vec<String>,
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
