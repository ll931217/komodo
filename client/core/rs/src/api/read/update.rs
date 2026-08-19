use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::{
  MongoDocument,
  update::{Update, UpdateListItem},
};

use super::KomodoReadRequest;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetUpdate",
  description = "Get all data for the target update.",
  request_body(content = GetUpdate),
  responses(
    (status = 200, description = "The update", body = GetUpdateResponse),
  ),
)]
pub fn get_update() {}

/// Get all data for the target update.
/// Response: [Update].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetUpdateResponse)]
#[error(mogh_error::Error)]
pub struct GetUpdate {
  /// The update id.
  pub id: String,
}

#[typeshare]
pub type GetUpdateResponse = Update;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ListUpdates",
  description = "Paginated endpoint for updates matching optional query.",
  request_body(content = ListUpdates),
  responses(
    (status = 200, description = "The paginated list of updates", body = ListUpdatesResponse),
  ),
)]
pub fn list_updates() {}

/// Paginated endpoint for updates matching optional query.
/// More recent updates will be returned first.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(ListUpdatesResponse)]
#[error(mogh_error::Error)]
pub struct ListUpdates {
  /// An optional mongo query to filter the updates.
  #[cfg_attr(feature = "utoipa", schema(value_type = Option<serde_json::Value>))]
  pub query: Option<MongoDocument>,
  /// Page of updates. Default is 0, which is the most recent data.
  /// Use with the `next_page` field of the response.
  #[serde(default)]
  pub page: u32,
}

/// Response for [ListUpdates].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ListUpdatesResponse {
  /// The page of updates, sorted by timestamp descending.
  pub updates: Vec<UpdateListItem>,
  /// If there is a next page of data, pass this to `page` to get it.
  pub next_page: Option<u32>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/GetUpdateRevertToml",
  description = "Get the config snapshot an Update can be reverted to.",
  request_body(content = GetUpdateRevertToml),
  responses(
    (status = 200, description = "The revert plan", body = GetUpdateRevertTomlResponse),
  ),
)]
pub fn get_update_revert_toml() {}

/// Get the config snapshot an Update can be reverted to.
///
/// Every config-changing Update stores the TOML from BEFORE the change
/// (`Update::prev_toml`). This hands that back, having checked it is
/// actually usable, so a revert is a reviewed action rather than a
/// blind one.
///
/// Deliberately a READ. It applies nothing. The returned TOML goes
/// through the normal sync path, which already diffs before applying and
/// records its own Update - so a revert is auditable and itself
/// revertible, and Komodo never rewrites production config off a single
/// click.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoReadRequest)]
#[response(GetUpdateRevertTomlResponse)]
#[error(mogh_error::Error)]
pub struct GetUpdateRevertToml {
  /// The Update to revert to the state BEFORE.
  pub update: String,
}

/// Response for [GetUpdateRevertToml].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetUpdateRevertTomlResponse {
  /// Whether this Update can be reverted to at all.
  pub revertable: bool,
  /// When not revertable, why - phrased for an operator, not a
  /// developer.
  pub reason: String,
  /// The config as it was before this Update. Empty when not
  /// revertable.
  pub toml: String,
  /// The config as it was after, for showing the diff being undone.
  pub current_toml: String,
}
