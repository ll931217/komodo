use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::{
  terraform::{_PartialTerraformConfig, Terraform},
  update::Update,
};

use super::KomodoWriteRequest;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/CreateTerraform",
  description = "Create a Terraform resource.",
  request_body(content = CreateTerraform),
  responses(
    (status = 200, description = "The new Terraform", body = crate::entities::terraform::TerraformSchema),
  ),
)]
pub fn create_terraform() {}

/// Create a Terraform resource. Response: [Terraform].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Terraform)]
#[error(mogh_error::Error)]
pub struct CreateTerraform {
  /// The name given to the newly created Terraform.
  pub name: String,
  /// Optional partial config to initialize the Terraform with.
  #[serde(default)]
  pub config: _PartialTerraformConfig,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/CopyTerraform",
  description = "Copy a Terraform resource.",
  request_body(content = CopyTerraform),
  responses(
    (status = 200, description = "The new Terraform", body = crate::entities::terraform::TerraformSchema),
  ),
)]
pub fn copy_terraform() {}

/// Creates a new Terraform with given `name` and the configuration
/// of the Terraform at the given `id`.
///
/// Note the copy runs against its own working directory and its own
/// managed state file, both keyed by name: it adopts nothing from the
/// original's infrastructure. Response: [Terraform].
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Terraform)]
#[error(mogh_error::Error)]
pub struct CopyTerraform {
  /// The name of the new Terraform.
  pub name: String,
  /// The id of the Terraform to copy.
  pub id: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DeleteTerraform",
  description = "Delete a Terraform resource.",
  request_body(content = DeleteTerraform),
  responses(
    (status = 200, description = "The deleted Terraform", body = crate::entities::terraform::TerraformSchema),
  ),
)]
pub fn delete_terraform() {}

/// Deletes the Terraform at the given id, and returns the deleted
/// Terraform.
///
/// This deletes the Komodo resource only. Whatever it applied stays
/// running, and its state file stays on the Server — run
/// [DestroyTerraform][super::super::execute::DestroyTerraform] first
/// to tear the infrastructure down. Response: [Terraform]
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Terraform)]
#[error(mogh_error::Error)]
pub struct DeleteTerraform {
  /// The id or name of the Terraform to delete.
  pub id: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/UpdateTerraform",
  description = "Update a Terraform resource.",
  request_body(content = UpdateTerraform),
  responses(
    (status = 200, description = "The updated Terraform", body = crate::entities::terraform::TerraformSchema),
  ),
)]
pub fn update_terraform() {}

/// Update the Terraform at the given id, and return the updated
/// Terraform. Response: [Terraform].
///
/// Note. This method updates only the fields which are set in the
/// [_PartialTerraformConfig], effectively merging diffs into the final
/// document. This is helpful when multiple users are using the same
/// resources concurrently by ensuring no unintentional field changes
/// occur from out of date local state.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(Terraform)]
#[error(mogh_error::Error)]
pub struct UpdateTerraform {
  /// The id of the Terraform to update.
  pub id: String,
  /// The partial config update to apply.
  pub config: _PartialTerraformConfig,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/RenameTerraform",
  description = "Rename a Terraform resource.",
  request_body(content = RenameTerraform),
  responses(
    (status = 200, description = "The update", body = crate::entities::update::Update),
  ),
)]
pub fn rename_terraform() {}

/// Rename the Terraform at id to the given name.
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
pub struct RenameTerraform {
  /// The id or name of the Terraform to rename.
  pub id: String,
  /// The new name.
  pub name: String,
}
