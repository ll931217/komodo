use anyhow::Context;
use database::mungos::mongodb::Collection;
use komodo_client::entities::{
  Operation, ResourceTarget, ResourceTargetVariant,
  cluster::{
    Cluster, ClusterConfig, ClusterConfigDiff, ClusterInfo,
    ClusterListItem, ClusterListItemInfo, ClusterQuerySpecifics,
    PartialClusterConfig,
  },
  permission::PermissionLevel,
  resource::Resource,
  server::Server,
  update::Update,
  user::User,
};

use crate::{
  config::core_config,
  monitor::refresh_cluster_cache,
  state::{cluster_status_cache, db_client},
};

use super::get_check_permissions;

impl super::KomodoResource for Cluster {
  type Config = ClusterConfig;
  type PartialConfig = PartialClusterConfig;
  type ConfigDiff = ClusterConfigDiff;
  type Info = ClusterInfo;
  type ListItem = ClusterListItem;
  type QuerySpecifics = ClusterQuerySpecifics;

  fn resource_type() -> ResourceTargetVariant {
    ResourceTargetVariant::Cluster
  }

  fn resource_target(id: impl Into<String>) -> ResourceTarget {
    ResourceTarget::Cluster(id.into())
  }

  fn coll() -> &'static Collection<Resource<Self::Config, Self::Info>>
  {
    &db_client().clusters
  }

  async fn to_list_item(
    cluster: Resource<Self::Config, Self::Info>,
  ) -> Self::ListItem {
    let (state, err) = cluster_status_cache()
      .get(&cluster.id)
      .await
      .map(|status| (status.state, status.err.clone()))
      .unwrap_or_default();
    ClusterListItem {
      name: cluster.name,
      id: cluster.id,
      template: cluster.template,
      tags: cluster.tags,
      resource_type: ResourceTargetVariant::Cluster,
      info: ClusterListItemInfo {
        server_id: cluster.config.server_id,
        context: cluster.config.context,
        namespace: cluster.config.namespace,
        state,
        err,
      },
    }
  }

  async fn busy(_id: &String) -> anyhow::Result<bool> {
    Ok(false)
  }

  // CREATE

  fn create_operation() -> Operation {
    Operation::CreateCluster
  }

  fn user_can_create(user: &User) -> bool {
    user.admin || !core_config().disable_non_admin_create
  }

  async fn validate_create_config(
    config: &mut Self::PartialConfig,
    user: &User,
  ) -> anyhow::Result<()> {
    validate_config(config, user).await
  }

  async fn post_create(
    created: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    refresh_cluster_cache(created, true).await;
    Ok(())
  }

  // UPDATE

  fn update_operation() -> Operation {
    Operation::UpdateCluster
  }

  async fn validate_update_config(
    _id: &str,
    config: &mut Self::PartialConfig,
    user: &User,
  ) -> anyhow::Result<()> {
    validate_config(config, user).await
  }

  async fn post_update(
    updated: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    refresh_cluster_cache(updated, true).await;
    Ok(())
  }

  // RENAME

  fn rename_operation() -> Operation {
    Operation::RenameCluster
  }

  // DELETE

  fn delete_operation() -> Operation {
    Operation::DeleteCluster
  }

  async fn pre_delete(
    _cluster: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  async fn post_delete(
    cluster: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    cluster_status_cache().remove(&cluster.id).await;
    Ok(())
  }
}

#[instrument("ValidateClusterConfig", skip_all)]
async fn validate_config(
  config: &mut PartialClusterConfig,
  user: &User,
) -> anyhow::Result<()> {
  // The Server holds the kubeconfig and runs kubectl for this Cluster,
  // so attaching one requires read access to it.
  if let Some(server_id) = &mut config.server_id {
    if !server_id.is_empty() {
      let server = get_check_permissions::<Server>(
        server_id,
        user,
        PermissionLevel::Read.attach(),
      )
      .await
      .with_context(|| {
        format!("Cannot attach Server {server_id} to this Cluster")
      })?;
      *server_id = server.id;
    }
  }
  // A default namespace outside the allow-list would make every
  // operation that relies on the default fail at execution time.
  let namespaces = config.namespaces.as_deref().unwrap_or_default();
  if let Some(namespace) = &config.namespace
    && !namespace.is_empty()
    && !namespaces.is_empty()
    && !namespaces.contains(namespace)
  {
    anyhow::bail!(
      "Default namespace '{namespace}' is not in the allowed namespaces {namespaces:?}"
    );
  }
  Ok(())
}
