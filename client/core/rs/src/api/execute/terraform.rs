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
  path = "/PlanTerraform",
  description = "Show what applying a Terraform resource would change.",
  request_body(content = PlanTerraform),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn plan_terraform() {}

/// Shows the changes applying would make, without making them.
/// `terraform init` then `terraform plan -detailed-exitcode`.
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
pub struct PlanTerraform {
  /// Id or name
  pub terraform: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchPlanTerraform",
  description = "Plans multiple Terraform resources in parallel that match pattern.",
  request_body(content = BatchPlanTerraform),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_plan_terraform() {}

/// Plans multiple Terraform resources in parallel that match pattern.
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
pub struct BatchPlanTerraform {
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
  path = "/ApplyTerraform",
  description = "Apply a Terraform resource.",
  request_body(content = ApplyTerraform),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn apply_terraform() {}

/// Applies the Terraform resource, creating and changing real
/// infrastructure. `terraform init` then
/// `terraform apply -auto-approve`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct ApplyTerraform {
  /// Id or name
  pub terraform: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchApplyTerraform",
  description = "Applies multiple Terraform resources in parallel that match pattern.",
  request_body(content = BatchApplyTerraform),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_apply_terraform() {}

/// Applies multiple Terraform resources in parallel that match
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
pub struct BatchApplyTerraform {
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
  path = "/DestroyTerraform",
  description = "Destroy everything a Terraform resource manages.",
  request_body(content = DestroyTerraform),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn destroy_terraform() {}

/// Destroys every resource in the unit's state.
/// `terraform init` then `terraform destroy -auto-approve`.
/// Requires Write permission rather than Execute: the blast radius is
/// everything the unit manages, the same reasoning as
/// [ApplyClusterObject][super::ApplyClusterObject].
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
pub struct DestroyTerraform {
  /// Id or name
  pub terraform: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchDestroyTerraform",
  description = "Destroys multiple Terraform resources in parallel that match pattern.",
  request_body(content = BatchDestroyTerraform),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_destroy_terraform() {}

/// Destroys multiple Terraform resources in parallel that match
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
pub struct BatchDestroyTerraform {
  /// Id or name or wildcard pattern or regex.
  /// Supports multiline and comma delineated combinations of the above.
  pub pattern: String,

  /// Filter matches by tag.
  /// If empty, skips tag filtering.
  #[serde(default)]
  pub tags: Vec<String>,
}
