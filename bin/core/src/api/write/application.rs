use komodo_client::{
  api::write::*,
  entities::{
    application::Application, permission::PermissionLevel,
    update::Update,
  },
};
use mogh_resolver::Resolve;

use crate::{permission::get_check_permissions, resource};

use super::WriteArgs;

impl Resolve<WriteArgs> for CreateApplication {
  #[instrument(
    "CreateApplication",
    skip_all,
    fields(
      operator = user.id,
      application = self.name,
      config = serde_json::to_string(&self.config).unwrap(),
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Application> {
    resource::create::<Application>(
      &self.name,
      self.config,
      None,
      user,
    )
    .await
  }
}

impl Resolve<WriteArgs> for CopyApplication {
  #[instrument(
    "CopyApplication",
    skip_all,
    fields(
      operator = user.id,
      application = self.name,
      copy_application = self.id,
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Application> {
    let Application { config, .. } =
      get_check_permissions::<Application>(
        &self.id,
        user,
        PermissionLevel::Read.into(),
      )
      .await?;
    resource::create::<Application>(
      &self.name,
      config.into(),
      None,
      user,
    )
    .await
  }
}

impl Resolve<WriteArgs> for DeleteApplication {
  #[instrument(
    "DeleteApplication",
    skip_all,
    fields(
      operator = user.id,
      application = self.id,
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Application> {
    Ok(resource::delete::<Application>(&self.id, user).await?)
  }
}

impl Resolve<WriteArgs> for UpdateApplication {
  #[instrument(
    "UpdateApplication",
    skip_all,
    fields(
      operator = user.id,
      application = self.id,
      update = serde_json::to_string(&self.config).unwrap()
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Application> {
    Ok(
      resource::update::<Application>(&self.id, self.config, user)
        .await?,
    )
  }
}

impl Resolve<WriteArgs> for RenameApplication {
  #[instrument(
    "RenameApplication",
    skip_all,
    fields(
      operator = user.id,
      application = self.id,
      new_name = self.name
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Update> {
    Ok(
      resource::rename::<Application>(&self.id, &self.name, user)
        .await?,
    )
  }
}
