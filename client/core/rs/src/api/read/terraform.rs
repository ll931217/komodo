use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::{
  U64,
  terraform::{
    Terraform, TerraformActionState, TerraformListItem,
    TerraformQuery, TerraformSortBy,
  },
};

use super::KomodoReadRequest;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetTerraform",
  description = "Get a specific Terraform resource.",
  request_body(content = GetTerraform),
  responses(
    (status = 200, description = "The Terraform", body = crate::entities::terraform::TerraformSchema),
  ),
)]
pub fn get_terraform() {}

/// Get a specific Terraform resource. Response: [Terraform].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetTerraformResponse)]
#[error(mogh_error::Error)]
pub struct GetTerraform {
  /// Id or name
  #[serde(alias = "id", alias = "name")]
  pub terraform: String,
}

#[typeshare]
pub type GetTerraformResponse = Terraform;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetTerraformActionState",
  description = "Get current action state for the Terraform resource.",
  request_body(content = GetTerraformActionState),
  responses(
    (status = 200, description = "The Terraform action state", body = GetTerraformActionStateResponse),
  ),
)]
pub fn get_terraform_action_state() {}

/// Get current action state for the Terraform resource.
/// Response: [TerraformActionState].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetTerraformActionStateResponse)]
#[error(mogh_error::Error)]
pub struct GetTerraformActionState {
  /// Id or name
  #[serde(alias = "id", alias = "name")]
  pub terraform: String,
}

#[typeshare]
pub type GetTerraformActionStateResponse = TerraformActionState;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ListTerraforms",
  description = "List Terraform resources matching optional query.",
  request_body(content = ListTerraforms),
  responses(
    (status = 200, description = "The list of Terraforms", body = ListTerraformsResponse),
  ),
)]
pub fn list_terraforms() {}

/// List Terraform resources matching optional query.
/// Response: [ListTerraformsResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(ListTerraformsResponse)]
#[error(mogh_error::Error)]
pub struct ListTerraforms {
  /// Structured query to filter Terraforms.
  #[serde(default)]
  pub query: TerraformQuery,

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
  pub sort_by: TerraformSortBy,

  /// Reverse the sort direction.
  #[serde(default)]
  pub sort_desc: bool,
}

#[typeshare]
pub type ListTerraformsResponse = Vec<TerraformListItem>;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ListFullTerraforms",
  description = "List full Terraform resources matching optional query.",
  request_body(content = ListFullTerraforms),
  responses(
    (status = 200, description = "The list of Terraforms", body = ListFullTerraformsResponse),
  ),
)]
pub fn list_full_terraforms() {}

/// List full Terraform resources matching optional query.
/// Response: [ListFullTerraformsResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(ListFullTerraformsResponse)]
#[error(mogh_error::Error)]
pub struct ListFullTerraforms {
  /// Structured query to filter Terraforms.
  #[serde(default)]
  pub query: TerraformQuery,

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
}

#[typeshare]
pub type ListFullTerraformsResponse = Vec<Terraform>;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetTerraformsSummary",
  description = "Gets a summary of data relating to all Terraform resources.",
  request_body(content = GetTerraformsSummary),
  responses(
    (status = 200, description = "The Terraforms summary", body = GetTerraformsSummaryResponse),
  ),
)]
pub fn get_terraforms_summary() {}

/// Gets a summary of data relating to all Terraform resources.
/// Response: [GetTerraformsSummaryResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetTerraformsSummaryResponse)]
#[error(mogh_error::Error)]
pub struct GetTerraformsSummary {}

/// Response for [GetTerraformsSummary]
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetTerraformsSummaryResponse {
  /// The total number of Terraform resources
  pub total: u32,
  /// The number whose last run left no pending changes.
  pub ok: u32,
  /// The number whose last plan found pending changes.
  pub drifted: u32,
  /// The number whose last run failed.
  pub failed: u32,
  /// The number that have never run.
  pub unknown: u32,
}
