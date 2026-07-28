use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::cluster::{
  Cluster, ClusterActionState, ClusterListItem, ClusterQuery,
};

use super::KomodoReadRequest;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetCluster",
  description = "Get a specific cluster.",
  request_body(content = GetCluster),
  responses(
    (status = 200, description = "The cluster", body = crate::entities::cluster::ClusterSchema),
  ),
)]
pub fn get_cluster() {}

/// Get a specific cluster. Response: [Cluster].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetClusterResponse)]
#[error(mogh_error::Error)]
pub struct GetCluster {
  /// Id or name
  #[serde(alias = "id", alias = "name")]
  pub cluster: String,
}

#[typeshare]
pub type GetClusterResponse = Cluster;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ListClusters",
  description = "List Clusters matching optional query.",
  request_body(content = ListClusters),
  responses(
    (status = 200, description = "The list of clusters", body = ListClustersResponse),
  ),
)]
pub fn list_clusters() {}

/// List Clusters matching optional query. Response: [ListClustersResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(ListClustersResponse)]
#[error(mogh_error::Error)]
pub struct ListClusters {
  /// Optional structured query to filter Clusters.
  #[serde(default)]
  pub query: ClusterQuery,
}

#[typeshare]
pub type ListClustersResponse = Vec<ClusterListItem>;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ListFullClusters",
  description = "List Clusters matching optional query.",
  request_body(content = ListFullClusters),
  responses(
    (status = 200, description = "The list of clusters", body = ListFullClustersResponse),
  ),
)]
pub fn list_full_clusters() {}

/// List Clusters matching optional query. Response: [ListFullClustersResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(ListFullClustersResponse)]
#[error(mogh_error::Error)]
pub struct ListFullClusters {
  /// optional structured query to filter clusters.
  #[serde(default)]
  pub query: ClusterQuery,
}

#[typeshare]
pub type ListFullClustersResponse = Vec<Cluster>;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetClusterActionState",
  description = "Get current action state for the cluster.",
  request_body(content = GetClusterActionState),
  responses(
    (status = 200, description = "The cluster action state", body = GetClusterActionStateResponse),
  ),
)]
pub fn get_cluster_action_state() {}

/// Get current action state for the cluster. Response: [ClusterActionState].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetClusterActionStateResponse)]
#[error(mogh_error::Error)]
pub struct GetClusterActionState {
  /// Id or name
  #[serde(alias = "id", alias = "name")]
  pub cluster: String,
}

#[typeshare]
pub type GetClusterActionStateResponse = ClusterActionState;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetClustersSummary",
  description = "Gets a summary of data relating to all clusters.",
  request_body(content = GetClustersSummary),
  responses(
    (status = 200, description = "The clusters summary", body = GetClustersSummaryResponse),
  ),
)]
pub fn get_clusters_summary() {}

/// Gets a summary of data relating to all clusters.
/// Response: [GetClustersSummaryResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetClustersSummaryResponse)]
#[error(mogh_error::Error)]
pub struct GetClustersSummary {}

/// Response for [GetClustersSummary]
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetClustersSummaryResponse {
  /// The total number of Clusters
  pub total: u32,
  /// The number of Clusters with Ok state.
  pub ok: u32,
  /// The number of Clusters with Unreachable state
  pub unreachable: u32,
  /// The number of Clusters with Unknown state
  pub unknown: u32,
}
