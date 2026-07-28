use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::{
  cluster::{_PartialClusterConfig, Cluster},
  update::Update,
};

use super::KomodoWriteRequest;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/CreateCluster",
  description = "Create a Cluster.",
  request_body(content = CreateCluster),
  responses(
    (status = 200, description = "The new cluster", body = crate::entities::cluster::ClusterSchema),
  ),
)]
pub fn create_cluster() {}

/// Create a Cluster. Response: [Cluster].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Cluster)]
#[error(mogh_error::Error)]
pub struct CreateCluster {
  /// The name given to newly created cluster.
  pub name: String,
  /// Optional partial config to initialize the cluster with.
  #[serde(default)]
  pub config: _PartialClusterConfig,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/CopyCluster",
  description = "Copy a Cluster.",
  request_body(content = CopyCluster),
  responses(
    (status = 200, description = "The new cluster", body = crate::entities::cluster::ClusterSchema),
  ),
)]
pub fn copy_cluster() {}

/// Creates a new Cluster with given `name` and the configuration
/// of the Cluster at the given `id`. Response: [Cluster].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Cluster)]
#[error(mogh_error::Error)]
pub struct CopyCluster {
  /// The name of the new cluster.
  pub name: String,
  /// The id of the cluster to copy.
  pub id: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DeleteCluster",
  description = "Delete a Cluster.",
  request_body(content = DeleteCluster),
  responses(
    (status = 200, description = "The deleted cluster", body = crate::entities::cluster::ClusterSchema),
  ),
)]
pub fn delete_cluster() {}

/// Deletes the Cluster at the given id, and returns the deleted Cluster.
/// Response: [Cluster]
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Cluster)]
#[error(mogh_error::Error)]
pub struct DeleteCluster {
  /// The id or name of the cluster to delete.
  pub id: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/UpdateCluster",
  description = "Update a Cluster.",
  request_body(content = UpdateCluster),
  responses(
    (status = 200, description = "The updated cluster", body = crate::entities::cluster::ClusterSchema),
  ),
)]
pub fn update_cluster() {}

/// Update the Cluster at the given id, and return the updated Cluster.
/// Response: [Cluster].
///
/// Note. This method updates only the fields which are set in the [_PartialClusterConfig],
/// effectively merging diffs into the final document.
/// This is helpful when multiple users are using
/// the same resources concurrently by ensuring no unintentional
/// field changes occur from out of date local state.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Cluster)]
#[error(mogh_error::Error)]
pub struct UpdateCluster {
  /// The id of the cluster to update.
  pub id: String,
  /// The partial config update to apply.
  pub config: _PartialClusterConfig,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/RenameCluster",
  description = "Rename a Cluster.",
  request_body(content = RenameCluster),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn rename_cluster() {}

/// Rename the Cluster at id to the given name.
/// Response: [Update].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct RenameCluster {
  /// The id or name of the Cluster to rename.
  pub id: String,
  /// The new name.
  pub name: String,
}
