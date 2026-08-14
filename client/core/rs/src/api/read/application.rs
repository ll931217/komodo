use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::{
  U64,
  application::{
    Application, ApplicationActionState, ApplicationListItem,
    ApplicationQuery, ApplicationSortBy,
  },
};

use super::KomodoReadRequest;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetApplication",
  description = "Get a specific Application.",
  request_body(content = GetApplication),
  responses(
    (status = 200, description = "The Application", body = crate::entities::application::ApplicationSchema),
  ),
)]
pub fn get_application() {}

/// Get a specific Application. Response: [Application].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetApplicationResponse)]
#[error(mogh_error::Error)]
pub struct GetApplication {
  /// Id or name
  #[serde(alias = "id", alias = "name")]
  pub application: String,
}

#[typeshare]
pub type GetApplicationResponse = Application;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetApplicationActionState",
  description = "Get current action state for the Application.",
  request_body(content = GetApplicationActionState),
  responses(
    (status = 200, description = "The Application action state", body = GetApplicationActionStateResponse),
  ),
)]
pub fn get_application_action_state() {}

/// Get current action state for the Application.
/// Response: [ApplicationActionState].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetApplicationActionStateResponse)]
#[error(mogh_error::Error)]
pub struct GetApplicationActionState {
  /// Id or name
  #[serde(alias = "id", alias = "name")]
  pub application: String,
}

#[typeshare]
pub type GetApplicationActionStateResponse = ApplicationActionState;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ListApplications",
  description = "List Applications matching optional query.",
  request_body(content = ListApplications),
  responses(
    (status = 200, description = "The list of Applications", body = ListApplicationsResponse),
  ),
)]
pub fn list_applications() {}

/// List Applications matching optional query.
/// Response: [ListApplicationsResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(ListApplicationsResponse)]
#[error(mogh_error::Error)]
pub struct ListApplications {
  /// Structured query to filter Applications.
  #[serde(default)]
  pub query: ApplicationQuery,

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
  pub sort_by: ApplicationSortBy,

  /// Reverse the sort direction.
  #[serde(default)]
  pub sort_desc: bool,
}

#[typeshare]
pub type ListApplicationsResponse = Vec<ApplicationListItem>;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ListFullApplications",
  description = "List full Applications matching optional query.",
  request_body(content = ListFullApplications),
  responses(
    (status = 200, description = "The list of Applications", body = ListFullApplicationsResponse),
  ),
)]
pub fn list_full_applications() {}

/// List full Applications matching optional query.
/// Response: [ListFullApplicationsResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(ListFullApplicationsResponse)]
#[error(mogh_error::Error)]
pub struct ListFullApplications {
  /// Structured query to filter Applications.
  #[serde(default)]
  pub query: ApplicationQuery,

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
pub type ListFullApplicationsResponse = Vec<Application>;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetApplicationsSummary",
  description = "Gets a summary of data relating to all Applications.",
  request_body(content = GetApplicationsSummary),
  responses(
    (status = 200, description = "The Applications summary", body = GetApplicationsSummaryResponse),
  ),
)]
pub fn get_applications_summary() {}

/// Gets a summary of data relating to all Applications.
/// Response: [GetApplicationsSummaryResponse].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetApplicationsSummaryResponse)]
#[error(mogh_error::Error)]
pub struct GetApplicationsSummary {}

/// Response for [GetApplicationsSummary]
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetApplicationsSummaryResponse {
  /// The total number of Applications
  pub total: u32,
  /// The number whose last Deploy succeeded, or whose last Diff found
  /// no differences.
  pub deployed: u32,
  /// The number whose last Diff found differences.
  pub drifted: u32,
  /// The number whose last execution failed.
  pub failed: u32,
  /// The number never deployed, or destroyed since.
  pub unknown: u32,
}
