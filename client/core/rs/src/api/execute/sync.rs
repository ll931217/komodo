use clap::Parser;
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::{ResourceTargetVariant, update::Update};

use super::KomodoExecuteRequest;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/RunSync",
  description = "Runs the target resource sync.",
  request_body(content = RunSync),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn run_sync() {}

/// Runs the target resource sync. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct RunSync {
  /// Id or name
  pub sync: String,
  /// Only execute sync on a specific resource type.
  /// Combine with `resource_id` to specify resource.
  pub resource_type: Option<ResourceTargetVariant>,
  /// Only execute sync on a specific resources.
  /// Combine with `resource_type` to specify resources.
  /// Supports name or id.
  pub resources: Option<Vec<String>>,
  /// Compute the changes and report them, without applying any of
  /// them.
  ///
  /// Distinct from the pending diff, which is a cached view: this runs
  /// the same code path a real sync runs, right up to the point of
  /// mutation, so what it reports is what that run would actually do.
  #[serde(default)]
  pub dry_run: bool,
  /// Confirm the deletions a `confirm_deletes` sync reported on a
  /// previous run, so this run applies them.
  ///
  /// Ignored unless the ResourceSync has `confirm_deletes` set - on
  /// every other sync, deletions apply as configured.
  #[serde(default)]
  pub confirm_deletes: bool,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/CancelSync",
  description = "Cancel a sync that is currently running.",
  request_body(content = CancelSync),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn cancel_sync() {}

/// Cancels a RunSync that is in flight.
///
/// Cooperative, not a kill: the run stops between resource batches, so
/// whatever it already applied stays applied and is recorded in the
/// Update. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct CancelSync {
  /// Id or name
  pub sync: String,
}
