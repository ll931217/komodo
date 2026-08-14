use anyhow::Context;
use database::mungos::mongodb::Collection;
use komodo_client::entities::{
  Operation, ResourceTarget, ResourceTargetVariant,
  application::{
    Application, ApplicationConfig, ApplicationConfigDiff,
    ApplicationInfo, ApplicationListItem, ApplicationListItemInfo,
    ApplicationQuerySpecifics, PartialApplicationConfig,
  },
  cluster::Cluster,
  permission::PermissionLevel,
  repo::Repo,
  resource::Resource,
  update::Update,
  user::User,
};

use crate::{
  config::core_config,
  state::{action_states, db_client},
};

use super::{get_check_permissions, redacted};

impl super::KomodoResource for Application {
  type Config = ApplicationConfig;
  type PartialConfig = PartialApplicationConfig;
  type ConfigDiff = ApplicationConfigDiff;
  type Info = ApplicationInfo;
  type ListItem = ApplicationListItem;
  type QuerySpecifics = ApplicationQuerySpecifics;

  fn resource_type() -> ResourceTargetVariant {
    ResourceTargetVariant::Application
  }

  fn resource_target(id: impl Into<String>) -> ResourceTarget {
    ResourceTarget::Application(id.into())
  }

  /// `webhook_secret` authenticates an inbound webhook, so knowing it
  /// is enough to trigger a Deploy - it should not fall out of a Read.
  ///
  /// `file_contents` is left alone: the manifests are the point of the
  /// resource, and secrets belong in `[[VARIABLE]]` references rather
  /// than inline.
  fn sanitize_config(config: &mut Self::Config) {
    config.webhook_secret = redacted(&config.webhook_secret);
  }

  fn coll() -> &'static Collection<Resource<Self::Config, Self::Info>>
  {
    &db_client().applications
  }

  async fn to_list_item(
    application: Resource<Self::Config, Self::Info>,
  ) -> Self::ListItem {
    ApplicationListItem {
      name: application.name,
      id: application.id,
      template: application.template,
      tags: application.tags,
      resource_type: ResourceTargetVariant::Application,
      info: ApplicationListItemInfo {
        cluster_id: application.config.cluster_id.clone(),
        namespace: application.config.namespace.clone(),
        source_kind: application.config.manifest_source(),
        // Whatever the last execution wrote. Nothing probes this:
        // answering "does the cluster still match" means a real
        // `kubectl diff`.
        state: application.info.state,
      },
    }
  }

  async fn busy(id: &String) -> anyhow::Result<bool> {
    action_states()
      .application
      .get(id)
      .await
      .unwrap_or_default()
      .busy()
  }

  // CREATE

  fn create_operation() -> Operation {
    Operation::CreateApplication
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

  /// Nothing to warm: an Application has no status cache, because it
  /// has no probe.
  async fn post_create(
    _created: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  // UPDATE

  fn update_operation() -> Operation {
    Operation::UpdateApplication
  }

  async fn validate_update_config(
    _id: &str,
    config: &mut Self::PartialConfig,
    user: &User,
  ) -> anyhow::Result<()> {
    validate_config(config, user).await
  }

  async fn post_update(
    _updated: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  // RENAME

  fn rename_operation() -> Operation {
    Operation::RenameApplication
  }

  // DELETE

  fn delete_operation() -> Operation {
    Operation::DeleteApplication
  }

  async fn pre_delete(
    _application: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  async fn post_delete(
    _application: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }
}

#[instrument("ValidateApplicationConfig", skip_all)]
async fn validate_config(
  config: &mut PartialApplicationConfig,
  user: &User,
) -> anyhow::Result<()> {
  // Attaching a Cluster hands this Application that Cluster's
  // credentials and lets it deploy into it, so it takes the same check
  // as attaching any other credential-bearing resource. Without this,
  // Execute on an Application would be enough to repoint it at
  // production.
  if let Some(cluster_id) = &mut config.cluster_id
    && !cluster_id.is_empty()
  {
    let cluster = get_check_permissions::<Cluster>(
      cluster_id,
      user,
      PermissionLevel::Read.attach(),
    )
    .await
    .with_context(|| {
      format!(
        "Cannot attach Cluster {cluster_id} to this Application"
      )
    })?;
    *cluster_id = cluster.id;
  }
  // Same reasoning for the Repo the manifests come from: without this,
  // Create on an Application would be enough to read manifests out of
  // any Repo in the instance, private ones included. Stack already
  // guards the identical field this way.
  if let Some(linked_repo) = &mut config.linked_repo
    && !linked_repo.is_empty()
  {
    let repo = get_check_permissions::<Repo>(
      linked_repo,
      user,
      PermissionLevel::Read.attach(),
    )
    .await
    .with_context(|| {
      format!("Cannot attach Repo {linked_repo} to this Application")
    })?;
    *linked_repo = repo.id;
  }
  Ok(())
}

#[cfg(test)]
mod sanitize_tests {
  use super::super::{KomodoResource, REDACTED};
  use super::*;

  /// Read must not hand out the webhook secret: knowing it is enough
  /// to trigger a Deploy.
  #[test]
  fn webhook_secret_is_redacted() {
    let mut config = ApplicationConfig {
      webhook_secret: String::from("s3cret"),
      ..Default::default()
    };
    <Application as KomodoResource>::sanitize_config(&mut config);
    assert_eq!(config.webhook_secret, REDACTED);
  }

  /// "Not configured" and "configured but hidden" must stay
  /// distinguishable, or debugging a failing webhook is guesswork.
  #[test]
  fn unset_fields_stay_empty() {
    let mut config = ApplicationConfig::default();
    <Application as KomodoResource>::sanitize_config(&mut config);
    assert!(config.webhook_secret.is_empty());
  }

  /// The manifests are the resource; redacting them would make Read
  /// useless for the thing people actually look at.
  #[test]
  fn manifests_are_not_redacted() {
    let mut config = ApplicationConfig {
      file_contents: String::from("kind: ConfigMap\n"),
      ..Default::default()
    };
    <Application as KomodoResource>::sanitize_config(&mut config);
    assert_eq!(config.file_contents, "kind: ConfigMap\n");
  }
}
