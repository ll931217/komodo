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
  path = "/DeployApplication",
  description = "Apply an Application's manifests to its Cluster.",
  request_body(content = DeployApplication),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn deploy_application() {}

/// Applies the Application's manifests to its Cluster.
/// `kubectl apply`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct DeployApplication {
  /// Id or name
  pub application: String,
  /// Override the Application's namespace for this deploy.
  /// Must be permitted by the Cluster's allowed namespaces.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchDeployApplication",
  description = "Deploys multiple Applications in parallel that match pattern.",
  request_body(content = BatchDeployApplication),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_deploy_application() {}

/// Deploys multiple Applications in parallel that match pattern.
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
pub struct BatchDeployApplication {
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
  path = "/DestroyApplication",
  description = "Delete the objects declared by an Application's manifests.",
  request_body(content = DestroyApplication),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn destroy_application() {}

/// Deletes the objects declared by the Application's manifests.
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
pub struct DestroyApplication {
  /// Id or name
  pub application: String,
  /// Override the Application's namespace for this delete.
  /// Must be permitted by the Cluster's allowed namespaces.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchDestroyApplication",
  description = "Destroys multiple Applications in parallel that match pattern.",
  request_body(content = BatchDestroyApplication),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_destroy_application() {}

/// Destroys multiple Applications in parallel that match pattern.
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
pub struct BatchDestroyApplication {
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
  path = "/DiffApplication",
  description = "Preview what deploying an Application would change.",
  request_body(content = DiffApplication),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn diff_application() {}

/// Shows what deploying the Application would change, without changing
/// anything. `kubectl diff`. Run it on a schedule for drift detection:
/// nothing polls this in the background, because answering it means a
/// real call to the cluster. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct DiffApplication {
  /// Id or name
  pub application: String,
  /// Override the Application's namespace for this diff.
  /// Must be permitted by the Cluster's allowed namespaces.
  #[serde(default)]
  pub namespace: Option<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchDiffApplication",
  description = "Diffs multiple Applications in parallel that match pattern.",
  request_body(content = BatchDiffApplication),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_diff_application() {}

/// Diffs multiple Applications in parallel that match pattern.
/// The shape a scheduled drift sweep runs as.
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
pub struct BatchDiffApplication {
  /// Id or name or wildcard pattern or regex.
  /// Supports multiline and comma delineated combinations of the above.
  pub pattern: String,

  /// Filter matches by tag.
  /// If empty, skips tag filtering.
  #[serde(default)]
  pub tags: Vec<String>,
}
