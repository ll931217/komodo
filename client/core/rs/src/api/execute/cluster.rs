use clap::Parser;
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::{U64, update::Update};

use super::{BatchExecutionResponse, KomodoExecuteRequest};

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DeployCluster",
  description = "Apply a Cluster's manifests.",
  request_body(content = DeployCluster),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn deploy_cluster() {}

/// Applies the Cluster's manifests. `kubectl apply`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct DeployCluster {
  /// Id or name
  pub cluster: String,
  /// Override the Cluster's default namespace for this apply.
  /// Must be permitted by the Cluster's allowed namespaces.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchDeployCluster",
  description = "Applies manifests for multiple Clusters in parallel that match pattern.",
  request_body(content = BatchDeployCluster),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_deploy_cluster() {}

/// Applies manifests for multiple Clusters in parallel that match
/// pattern. Response: [BatchExecutionResponse].
#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, PartialEq, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(BatchExecutionResponse)]
#[error(mogh_error::Error)]
pub struct BatchDeployCluster {
  /// Id or name or wildcard pattern or regex.
  /// Supports multiline and comma delineated combinations of the above.
  pub pattern: String,

  /// Filter matches by tag.
  /// If empty, skips tag filtering.
  #[serde(default)]
  pub tags: Vec<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DestroyCluster",
  description = "Delete the objects declared by a Cluster's manifests.",
  request_body(content = DestroyCluster),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn destroy_cluster() {}

/// Deletes the objects declared by the Cluster's manifests.
/// `kubectl delete`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct DestroyCluster {
  /// Id or name
  pub cluster: String,
  /// Override the Cluster's default namespace for this delete.
  /// Must be permitted by the Cluster's allowed namespaces.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchDestroyCluster",
  description = "Destroys multiple Clusters in parallel that match pattern.",
  request_body(content = BatchDestroyCluster),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_destroy_cluster() {}

/// Destroys multiple Clusters in parallel that match pattern.
/// Response: [BatchExecutionResponse].
#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, PartialEq, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(BatchExecutionResponse)]
#[error(mogh_error::Error)]
pub struct BatchDestroyCluster {
  /// Id or name or wildcard pattern or regex.
  /// Supports multiline and comma delineated combinations of the above.
  pub pattern: String,

  /// Filter matches by tag.
  /// If empty, skips tag filtering.
  #[serde(default)]
  pub tags: Vec<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DiffCluster",
  description = "Preview what applying a Cluster's manifests would change.",
  request_body(content = DiffCluster),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn diff_cluster() {}

/// Shows what applying the Cluster's manifests would change, without
/// changing anything. `kubectl diff`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct DiffCluster {
  /// Id or name
  pub cluster: String,
  /// Override the Cluster's default namespace for this diff.
  /// Must be permitted by the Cluster's allowed namespaces.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DeleteClusterObject",
  description = "Delete a single Kubernetes object on a Cluster.",
  request_body(content = DeleteClusterObject),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn delete_cluster_object() {}

/// Deletes a single Kubernetes object by name. `kubectl delete`.
/// Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct DeleteClusterObject {
  /// Id or name
  pub cluster: String,
  /// Kubernetes kind, as kubectl accepts it (`pods`, `deployments`).
  pub kind: String,
  /// The object's name.
  pub name: String,
  /// Namespace the object lives in.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/RestartClusterWorkload",
  description = "Rolling-restart a workload on a Cluster.",
  request_body(content = RestartClusterWorkload),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn restart_cluster_workload() {}

/// Rolling-restart of a deployment / statefulset / daemonset.
/// `kubectl rollout restart`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct RestartClusterWorkload {
  /// Id or name
  pub cluster: String,
  /// `deployments`, `statefulsets` or `daemonsets`.
  pub kind: String,
  /// The workload's name.
  pub name: String,
  /// Namespace the workload lives in.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/RollbackClusterWorkload",
  description = "Roll a workload back to its previous revision.",
  request_body(content = RollbackClusterWorkload),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn rollback_cluster_workload() {}

/// Roll a deployment / statefulset / daemonset back to its previous
/// revision. `kubectl rollout undo`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct RollbackClusterWorkload {
  /// Id or name
  pub cluster: String,
  /// `deployments`, `statefulsets` or `daemonsets`.
  pub kind: String,
  /// The workload's name.
  pub name: String,
  /// Namespace the workload lives in.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ScaleClusterWorkload",
  description = "Scale a workload to a replica count.",
  request_body(content = ScaleClusterWorkload),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn scale_cluster_workload() {}

/// Scale a deployment / statefulset / replicaset to a replica count.
/// `kubectl scale`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct ScaleClusterWorkload {
  /// Id or name
  pub cluster: String,
  /// `deployments`, `statefulsets` or `replicasets`.
  pub kind: String,
  /// The workload's name.
  pub name: String,
  /// The desired replica count.
  pub replicas: u32,
  /// Namespace the workload lives in.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/CordonClusterNode",
  description = "Mark a node unschedulable.",
  request_body(content = CordonClusterNode),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn cordon_cluster_node() {}

/// Mark a node unschedulable. `kubectl cordon`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct CordonClusterNode {
  /// Id or name
  pub cluster: String,
  /// The node's name.
  pub node: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/UncordonClusterNode",
  description = "Mark a node schedulable again.",
  request_body(content = UncordonClusterNode),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn uncordon_cluster_node() {}

/// Mark a node schedulable again. `kubectl uncordon`.
/// Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct UncordonClusterNode {
  /// Id or name
  pub cluster: String,
  /// The node's name.
  pub node: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DrainClusterNode",
  description = "Drain a node in preparation for maintenance.",
  request_body(content = DrainClusterNode),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn drain_cluster_node() {}

/// Drain a node in preparation for maintenance. Cordons it and evicts
/// its pods. `kubectl drain`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct DrainClusterNode {
  /// Id or name
  pub cluster: String,
  /// The node's name.
  pub node: String,
  /// Evict pods even when they are not managed by a controller.
  #[serde(default)]
  #[clap(long)]
  pub force: bool,
  /// Continue when pods use emptyDir volumes (their data is deleted).
  #[serde(default)]
  #[clap(long)]
  pub delete_emptydir_data: bool,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ApplyClusterObject",
  description = "Apply a single edited Kubernetes object.",
  request_body(content = ApplyClusterObject),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn apply_cluster_object() {}

/// Apply a single edited Kubernetes object (YAML or JSON).
/// `kubectl apply -f`. Requires Write permission on the Cluster, since
/// arbitrary manifests are a wider grant than Execute.
/// Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct ApplyClusterObject {
  /// Id or name
  pub cluster: String,
  /// The object manifest, YAML or JSON.
  pub contents: String,
  /// Namespace to apply into.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/RollbackHelmRelease",
  description = "Roll a helm release back to a previous revision.",
  request_body(content = RollbackHelmRelease),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn rollback_helm_release() {}

/// Roll a helm release back. `helm rollback`. Without a revision,
/// helm rolls back to the previous one. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct RollbackHelmRelease {
  /// Id or name
  pub cluster: String,
  /// The release name.
  pub name: String,
  /// The release's namespace.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
  /// The revision to roll back to.
  /// Defaults to the previous revision.
  #[serde(default)]
  pub revision: Option<U64>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/UninstallHelmRelease",
  description = "Uninstall a helm release.",
  request_body(content = UninstallHelmRelease),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn uninstall_helm_release() {}

/// Uninstall a helm release. `helm uninstall`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct UninstallHelmRelease {
  /// Id or name
  pub cluster: String,
  /// The release name.
  pub name: String,
  /// The release's namespace.
  /// Defaults to the Cluster's default namespace.
  #[serde(default)]
  pub namespace: Option<String>,
}
