use komodo_client::{
  api::write::*,
  entities::{
    cluster::Cluster, permission::PermissionLevel, update::Update,
  },
};
use mogh_resolver::Resolve;

use crate::{permission::get_check_permissions, resource};

use super::WriteArgs;

impl Resolve<WriteArgs> for CreateCluster {
  #[instrument(
    "CreateCluster",
    skip_all,
    fields(
      operator = user.id,
      cluster = self.name,
      config = serde_json::to_string(&self.config).unwrap(),
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Cluster> {
    resource::create::<Cluster>(&self.name, self.config, None, user)
      .await
  }
}

impl Resolve<WriteArgs> for CopyCluster {
  #[instrument(
    "CopyCluster",
    skip_all,
    fields(
      operator = user.id,
      cluster = self.name,
      copy_cluster = self.id,
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Cluster> {
    let Cluster { config, .. } = get_check_permissions::<Cluster>(
      &self.id,
      user,
      PermissionLevel::Read.into(),
    )
    .await?;
    resource::create::<Cluster>(&self.name, config.into(), None, user)
      .await
  }
}

impl Resolve<WriteArgs> for DeleteCluster {
  #[instrument(
    "DeleteCluster",
    skip_all,
    fields(
      operator = user.id,
      cluster = self.id,
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Cluster> {
    Ok(resource::delete::<Cluster>(&self.id, user).await?)
  }
}

impl Resolve<WriteArgs> for UpdateCluster {
  #[instrument(
    "UpdateCluster",
    skip_all,
    fields(
      operator = user.id,
      cluster = self.id,
      update = serde_json::to_string(&self.config).unwrap()
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Cluster> {
    Ok(
      resource::update::<Cluster>(&self.id, self.config, user)
        .await?,
    )
  }
}

impl Resolve<WriteArgs> for RenameCluster {
  #[instrument(
    "RenameCluster",
    skip_all,
    fields(
      operator = user.id,
      cluster = self.id,
      new_name = self.name
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Update> {
    Ok(resource::rename::<Cluster>(&self.id, &self.name, user).await?)
  }
}
