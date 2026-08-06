use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::{
  JsonValue, SearchCombinator, U64,
  cluster::{
    Cluster, ClusterActionState, ClusterListItem,
    ClusterMetricsEntry, ClusterMetricsKind, ClusterPortForward,
    ClusterQuery, ClusterSortBy,
  },
  update::Log,
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

  /// Retrieve more results by incrementing the page.
  /// `page: 0` is default.
  #[serde(default)]
  pub page: U64,

  /// Set the limit for number of resources per-page.
  /// If not provided, uses the Core config
  /// `default_pagination_limit` (default: 30).
  ///
  /// Passing `limit: 0` returns all results (unlimited).
  ///
  /// Note: the page logic relies on this being consistent
  /// across queries for more pages.
  pub limit: Option<U64>,

  /// Sort the results by this field.
  /// Defaults to Name. Non-Name sorts are applied in memory
  /// after querying all matching resources.
  #[serde(default)]
  pub sort_by: ClusterSortBy,

  /// Reverse the sort direction.
  #[serde(default)]
  pub sort_desc: bool,
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

  /// Retrieve more results by incrementing the page.
  /// `page: 0` is default.
  #[serde(default)]
  pub page: U64,

  /// Set the limit for number of resources per-page.
  /// If not provided, uses the Core config
  /// `default_pagination_limit` (default: 30).
  ///
  /// Passing `limit: 0` returns all results (unlimited).
  pub limit: Option<U64>,
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

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ListClusterResources",
  description = "List Kubernetes objects of a kind on a Cluster.",
  request_body(content = ListClusterResources),
  responses(
    (status = 200, description = "The objects as opaque json", body = ListClusterResourcesResponse),
  ),
)]
pub fn list_cluster_resources() {}

/// List Kubernetes objects of a kind on a Cluster.
///
/// Komodo does not model Kubernetes types: the response is whatever
/// `kubectl get -o json` produced, for the UI to render generically.
/// Response: [ListClusterResourcesResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(ListClusterResourcesResponse)]
#[error(mogh_error::Error)]
pub struct ListClusterResources {
  /// Id or name
  pub cluster: String,
  /// Kubernetes kind, as kubectl accepts it (`pods`, `deployments`).
  pub kind: String,
  /// Namespace to read. Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
  /// Read across every allowed namespace.
  /// Rejected when the Cluster restricts namespaces.
  #[serde(default)]
  pub all_namespaces: bool,
}

#[typeshare]
pub type ListClusterResourcesResponse = JsonValue;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetClusterMetrics",
  description = "Get `kubectl top` node / pod usage on a Cluster.",
  request_body(content = GetClusterMetrics),
  responses(
    (status = 200, description = "The usage rows", body = GetClusterMetricsResponse),
  ),
)]
pub fn get_cluster_metrics() {}

/// Get `kubectl top` node / pod usage on a Cluster.
///
/// Requires the metrics-server to be installed on the cluster.
/// Response: [GetClusterMetricsResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetClusterMetricsResponse)]
#[error(mogh_error::Error)]
pub struct GetClusterMetrics {
  /// Id or name
  pub cluster: String,
  /// Measure pods or nodes.
  #[serde(default)]
  pub kind: ClusterMetricsKind,
  /// Namespace to read (pods only).
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
  /// Read across every allowed namespace (pods only).
  /// Rejected when the Cluster restricts namespaces.
  #[serde(default)]
  pub all_namespaces: bool,
}

#[typeshare]
pub type GetClusterMetricsResponse = Vec<ClusterMetricsEntry>;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ListHelmReleases",
  description = "List helm releases on a Cluster.",
  request_body(content = ListHelmReleases),
  responses(
    (status = 200, description = "The releases as opaque json", body = ListHelmReleasesResponse),
  ),
)]
pub fn list_helm_releases() {}

/// List helm releases on a Cluster.
///
/// Komodo does not model helm types: the response is whatever
/// `helm list -o json` produced.
/// Response: [ListHelmReleasesResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(ListHelmReleasesResponse)]
#[error(mogh_error::Error)]
pub struct ListHelmReleases {
  /// Id or name
  pub cluster: String,
  /// Namespace to list from.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
  /// List across every allowed namespace.
  /// Rejected when the Cluster restricts namespaces.
  #[serde(default)]
  pub all_namespaces: bool,
}

#[typeshare]
pub type ListHelmReleasesResponse = JsonValue;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/InspectHelmRelease",
  description = "Get a helm release's history and values.",
  request_body(content = InspectHelmRelease),
  responses(
    (status = 200, description = "History and values as opaque json", body = InspectHelmReleaseResponse),
  ),
)]
pub fn inspect_helm_release() {}

/// Get a helm release's revision history and user-supplied values,
/// as `{ "history": [...], "values": {...} }`.
/// Response: [InspectHelmReleaseResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(InspectHelmReleaseResponse)]
#[error(mogh_error::Error)]
pub struct InspectHelmRelease {
  /// Id or name
  pub cluster: String,
  /// The release name.
  pub name: String,
  /// The release's namespace.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
}

#[typeshare]
pub type InspectHelmReleaseResponse = JsonValue;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ListClusterPortForwards",
  description = "List the port-forward sessions on a Cluster.",
  request_body(content = ListClusterPortForwards),
  responses(
    (status = 200, description = "The sessions", body = ListClusterPortForwardsResponse),
  ),
)]
pub fn list_cluster_port_forwards() {}

/// List the `kubectl port-forward` sessions running on the Cluster's
/// Server. Response: [ListClusterPortForwardsResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(ListClusterPortForwardsResponse)]
#[error(mogh_error::Error)]
pub struct ListClusterPortForwards {
  /// Id or name
  pub cluster: String,
}

#[typeshare]
pub type ListClusterPortForwardsResponse = Vec<ClusterPortForward>;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/InspectClusterResource",
  description = "Get a single Kubernetes object as json.",
  request_body(content = InspectClusterResource),
  responses(
    (status = 200, description = "The object as opaque json", body = InspectClusterResourceResponse),
  ),
)]
pub fn inspect_cluster_resource() {}

/// Get a single Kubernetes object as json.
/// Response: [InspectClusterResourceResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(InspectClusterResourceResponse)]
#[error(mogh_error::Error)]
pub struct InspectClusterResource {
  /// Id or name
  pub cluster: String,
  pub kind: String,
  pub name: String,
  /// Namespace to read. Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
}

#[typeshare]
pub type InspectClusterResourceResponse = JsonValue;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetClusterPodLog",
  description = "Get a pod's log.",
  request_body(content = GetClusterPodLog),
  responses(
    (status = 200, description = "The log", body = crate::entities::update::Log),
  ),
)]
pub fn get_cluster_pod_log() {}

/// Get a pod's log. Response: [Log].
///
/// Requires the `Logs` specific permission on the Cluster.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetClusterPodLogResponse)]
#[error(mogh_error::Error)]
pub struct GetClusterPodLog {
  /// Id or name
  pub cluster: String,
  /// The pod's name.
  pub pod: String,
  /// Which container in the pod. Required only for multi-container
  /// pods; the sole container is used otherwise.
  #[serde(default)]
  pub container: Option<String>,
  /// Namespace the pod lives in.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
  /// How many lines from the end to return. Default 100.
  #[serde(default)]
  pub tail: Option<U64>,
  /// Include logs from the previous, terminated instance of the
  /// container - the only way to see why a crashlooping pod died.
  #[serde(default)]
  pub previous: bool,
  /// Enable `--timestamps`
  #[serde(default)]
  pub timestamps: bool,
}

#[typeshare]
pub type GetClusterPodLogResponse = Log;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/SearchClusterPodLog",
  description = "Search a pod log's tail using `grep`.",
  request_body(content = SearchClusterPodLog),
  responses(
    (status = 200, description = "The search results", body = SearchClusterPodLogResponse),
  ),
)]
pub fn search_cluster_pod_log() {}

/// Search a pod log's tail using `grep`. All lines go to stdout.
/// Response: [Log].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(SearchClusterPodLogResponse)]
#[error(mogh_error::Error)]
pub struct SearchClusterPodLog {
  /// Id or name
  pub cluster: String,
  /// The pod's name.
  pub pod: String,
  /// Which container in the pod. Required only for multi-container
  /// pods; the sole container is used otherwise.
  #[serde(default)]
  pub container: Option<String>,
  /// Namespace the pod lives in.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
  /// The terms to search for.
  pub terms: Vec<String>,
  /// When searching for multiple terms, can use `AND` or `OR` combinator.
  ///
  /// - `AND`: Only include lines with **all** terms present in that line.
  /// - `OR`: Include lines that have one or more matches in the terms.
  #[serde(default)]
  pub combinator: SearchCombinator,
  /// Invert the results, ie return all lines that DON'T match the terms / combinator.
  #[serde(default)]
  pub invert: bool,
  /// Enable `--timestamps`
  #[serde(default)]
  pub timestamps: bool,
}

#[typeshare]
pub type SearchClusterPodLogResponse = Log;
