use clap::Parser;
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::update::Update;

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
