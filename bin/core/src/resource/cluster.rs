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
  state::{action_states, cluster_status_cache, db_client},
};

use super::{get_check_permissions, redacted};

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

  /// `kubeconfig_contents` usually IS a cluster-admin credential, and
  /// `webhook_secret` is what authenticates an inbound webhook — neither
  /// should fall out of a Read.
  ///
  /// `kubeconfig_path` is left alone: it names a file on the Periphery
  /// host rather than carrying the credential, and it is load-bearing for
  /// understanding how a Cluster is wired.
  fn sanitize_config(config: &mut Self::Config) {
    config.kubeconfig_contents =
      redacted(&config.kubeconfig_contents);
    config.webhook_secret = redacted(&config.webhook_secret);
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

  async fn busy(id: &String) -> anyhow::Result<bool> {
    action_states()
      .cluster
      .get(id)
      .await
      .unwrap_or_default()
      .busy()
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

#[cfg(test)]
mod sanitize_tests {
  use super::super::{KomodoResource, REDACTED, redacted};
  use super::*;

  /// Read on a Cluster must not yield the kubeconfig — it is usually a
  /// cluster-admin credential, which is strictly more than Read.
  #[test]
  fn kubeconfig_and_webhook_secret_are_redacted() {
    let mut config = ClusterConfig {
      kubeconfig_contents: "apiVersion: v1\nclusters:\n- cluster:\n    server: https://10.0.0.1:6443"
        .to_string(),
      webhook_secret: "s3cret".to_string(),
      ..Default::default()
    };
    <Cluster as KomodoResource>::sanitize_config(&mut config);
    assert_eq!(config.kubeconfig_contents, REDACTED);
    assert_eq!(config.webhook_secret, REDACTED);
  }

  /// "Not configured" and "configured but hidden" must stay distinguishable,
  /// or debugging a Cluster that will not connect becomes guesswork.
  #[test]
  fn unset_fields_stay_empty() {
    let mut config = ClusterConfig::default();
    <Cluster as KomodoResource>::sanitize_config(&mut config);
    assert!(config.kubeconfig_contents.is_empty());
    assert!(config.webhook_secret.is_empty());
  }

  /// The marker must not encode how long the secret was.
  #[test]
  fn redaction_does_not_leak_length() {
    assert_eq!(redacted("a"), redacted(&"a".repeat(4096)));
    assert!(redacted("").is_empty());
  }

  /// kubeconfig_path names a file on the Periphery host rather than
  /// carrying the credential, and it explains how a Cluster is wired.
  #[test]
  fn kubeconfig_path_is_preserved() {
    let mut config = ClusterConfig {
      kubeconfig_path: "/etc/kubernetes/admin.conf".to_string(),
      ..Default::default()
    };
    <Cluster as KomodoResource>::sanitize_config(&mut config);
    assert_eq!(config.kubeconfig_path, "/etc/kubernetes/admin.conf");
  }
}
