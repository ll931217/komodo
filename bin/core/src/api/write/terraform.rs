use komodo_client::{
  api::write::*,
  entities::{
    permission::PermissionLevel, terraform::Terraform, update::Update,
  },
};
use mogh_resolver::Resolve;

use crate::{permission::get_check_permissions, resource};

use super::WriteArgs;

impl Resolve<WriteArgs> for CreateTerraform {
  #[instrument(
    "CreateTerraform",
    skip_all,
    fields(
      operator = user.id,
      terraform = self.name,
      config = serde_json::to_string(&self.config).unwrap(),
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Terraform> {
    resource::create::<Terraform>(&self.name, self.config, None, user)
      .await
  }
}

impl Resolve<WriteArgs> for CopyTerraform {
  #[instrument(
    "CopyTerraform",
    skip_all,
    fields(
      operator = user.id,
      terraform = self.name,
      copy_terraform = self.id,
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Terraform> {
    let Terraform { config, .. } =
      get_check_permissions::<Terraform>(
        &self.id,
        user,
        PermissionLevel::Read.into(),
      )
      .await?;
    resource::create::<Terraform>(
      &self.name,
      config.into(),
      None,
      user,
    )
    .await
  }
}

impl Resolve<WriteArgs> for DeleteTerraform {
  #[instrument(
    "DeleteTerraform",
    skip_all,
    fields(
      operator = user.id,
      terraform = self.id,
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Terraform> {
    Ok(resource::delete::<Terraform>(&self.id, user).await?)
  }
}

impl Resolve<WriteArgs> for UpdateTerraform {
  #[instrument(
    "UpdateTerraform",
    skip_all,
    fields(
      operator = user.id,
      terraform = self.id,
      update = serde_json::to_string(&self.config).unwrap()
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Terraform> {
    Ok(
      resource::update::<Terraform>(&self.id, self.config, user)
        .await?,
    )
  }
}

impl Resolve<WriteArgs> for RenameTerraform {
  #[instrument(
    "RenameTerraform",
    skip_all,
    fields(
      operator = user.id,
      terraform = self.id,
      new_name = self.name
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Update> {
    Ok(
      resource::rename::<Terraform>(&self.id, &self.name, user)
        .await?,
    )
  }
}
