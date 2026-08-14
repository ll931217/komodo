use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::{
  application::{_PartialApplicationConfig, Application},
  update::Update,
};

use super::KomodoWriteRequest;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/CreateApplication",
  description = "Create an Application.",
  request_body(content = CreateApplication),
  responses(
    (status = 200, description = "The new Application", body = crate::entities::application::ApplicationSchema),
  ),
)]
pub fn create_application() {}

/// Create an Application. Response: [Application].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Application)]
#[error(mogh_error::Error)]
pub struct CreateApplication {
  /// The name given to the newly created Application.
  pub name: String,
  /// Optional partial config to initialize the Application with.
  #[serde(default)]
  pub config: _PartialApplicationConfig,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/CopyApplication",
  description = "Copy an Application.",
  request_body(content = CopyApplication),
  responses(
    (status = 200, description = "The new Application", body = crate::entities::application::ApplicationSchema),
  ),
)]
pub fn copy_application() {}

/// Creates a new Application with given `name` and the configuration
/// of the Application at the given `id`.
///
/// Note the copy runs against its own working directory and its own
/// managed state file, both keyed by name: it adopts nothing from the
/// original's infrastructure. Response: [Application].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Application)]
#[error(mogh_error::Error)]
pub struct CopyApplication {
  /// The name of the new Application.
  pub name: String,
  /// The id of the Application to copy.
  pub id: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DeleteApplication",
  description = "Delete an Application.",
  request_body(content = DeleteApplication),
  responses(
    (status = 200, description = "The deleted Application", body = crate::entities::application::ApplicationSchema),
  ),
)]
pub fn delete_application() {}

/// Deletes the Application at the given id, and returns the deleted
/// Application.
///
/// This deletes the Komodo resource only. Whatever it applied stays
/// running, and its state file stays on the Server — run
/// [DestroyApplication][super::super::execute::DestroyApplication] first
/// to tear the infrastructure down. Response: [Application]
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Application)]
#[error(mogh_error::Error)]
pub struct DeleteApplication {
  /// The id or name of the Application to delete.
  pub id: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/UpdateApplication",
  description = "Update an Application.",
  request_body(content = UpdateApplication),
  responses(
    (status = 200, description = "The updated Application", body = crate::entities::application::ApplicationSchema),
  ),
)]
pub fn update_application() {}

/// Update the Application at the given id, and return the updated
/// Application. Response: [Application].
///
/// Note. This method updates only the fields which are set in the
/// [_PartialApplicationConfig], effectively merging diffs into the final
/// document. This is helpful when multiple users are using the same
/// resources concurrently by ensuring no unintentional field changes
/// occur from out of date local state.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Application)]
#[error(mogh_error::Error)]
pub struct UpdateApplication {
  /// The id of the Application to update.
  pub id: String,
  /// The partial config update to apply.
  pub config: _PartialApplicationConfig,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/RenameApplication",
  description = "Rename an Application.",
  request_body(content = RenameApplication),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn rename_application() {}

/// Rename the Application at id to the given name.
///
/// The name keys the working directory and the managed state file on
/// the Server, so the next run after a rename starts from an empty
/// state file rather than adopting what the old name applied.
/// Response: [Update].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct RenameApplication {
  /// The id or name of the Application to rename.
  pub id: String,
  /// The new name.
  pub name: String,
}
