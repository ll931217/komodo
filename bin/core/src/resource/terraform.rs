use anyhow::Context;
use database::mungos::mongodb::Collection;
use komodo_client::entities::{
  Operation, ResourceTarget, ResourceTargetVariant,
  cluster::Cluster,
  permission::PermissionLevel,
  resource::Resource,
  server::Server,
  terraform::{
    PartialTerraformConfig, Terraform, TerraformConfig,
    TerraformConfigDiff, TerraformInfo, TerraformListItem,
    TerraformListItemInfo, TerraformQuerySpecifics, TerraformState,
  },
  update::Update,
  user::User,
};

use crate::{
  config::core_config,
  state::{action_states, db_client},
};

use super::{get_check_permissions, redacted};

impl super::KomodoResource for Terraform {
  type Config = TerraformConfig;
  type PartialConfig = PartialTerraformConfig;
  type ConfigDiff = TerraformConfigDiff;
  type Info = TerraformInfo;
  type ListItem = TerraformListItem;
  type QuerySpecifics = TerraformQuerySpecifics;

  fn resource_type() -> ResourceTargetVariant {
    ResourceTargetVariant::Terraform
  }

  fn resource_target(id: impl Into<String>) -> ResourceTarget {
    ResourceTarget::Terraform(id.into())
  }

  /// `environment` carries `TF_VAR_*` provider credentials, and
  /// `webhook_secret` authenticates an inbound webhook — neither
  /// should fall out of a Read.
  ///
  /// `file_contents` is left alone: it is the terraform source, which
  /// is the point of the resource, and secrets belong in
  /// `[[VARIABLE]]` references rather than inline.
  fn sanitize_config(config: &mut Self::Config) {
    config.environment = redacted(&config.environment);
    config.webhook_secret = redacted(&config.webhook_secret);
  }

  fn coll() -> &'static Collection<Resource<Self::Config, Self::Info>>
  {
    &db_client().terraforms
  }

  async fn to_list_item(
    terraform: Resource<Self::Config, Self::Info>,
  ) -> Self::ListItem {
    TerraformListItem {
      name: terraform.name,
      id: terraform.id,
      template: terraform.template,
      tags: terraform.tags,
      resource_type: ResourceTargetVariant::Terraform,
      info: TerraformListItemInfo {
        server_id: terraform.config.server_id.clone(),
        run_directory: terraform.config.run_directory.clone(),
        source_kind: terraform.config.source_kind(),
        // Asking terraform for the truth means running a plan, which
        // is a real execution — so there is no probe to read here.
        // The execute APIs derive this from the last run's outcome.
        state: TerraformState::Unknown,
      },
    }
  }

  async fn busy(id: &String) -> anyhow::Result<bool> {
    action_states()
      .terraform
      .get(id)
      .await
      .unwrap_or_default()
      .busy()
  }

  // CREATE

  fn create_operation() -> Operation {
    Operation::CreateTerraform
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

  /// Nothing to warm: there is no status cache for Terraform, because
  /// there is no probe that does not cost a real terraform run.
  async fn post_create(
    _created: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  // UPDATE

  fn update_operation() -> Operation {
    Operation::UpdateTerraform
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
    Operation::RenameTerraform
  }

  // DELETE

  fn delete_operation() -> Operation {
    Operation::DeleteTerraform
  }

  async fn pre_delete(
    _terraform: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  async fn post_delete(
    _terraform: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }
}

#[instrument("ValidateTerraformConfig", skip_all)]
async fn validate_config(
  config: &mut PartialTerraformConfig,
  user: &User,
) -> anyhow::Result<()> {
  // The Server's Periphery runs terraform for this resource, so
  // attaching one requires read access to it.
  if let Some(server_id) = &mut config.server_id {
    if !server_id.is_empty() {
      let server = get_check_permissions::<Server>(
        server_id,
        user,
        PermissionLevel::Read.attach(),
      )
      .await
      .with_context(|| {
        format!(
          "Cannot attach Server {server_id} to this Terraform resource"
        )
      })?;
      *server_id = server.id;
    }
  }
  // The bridged Cluster's kubeconfig is materialized for the run, so
  // attaching one is a credential grant and needs the same check.
  if let Some(cluster_id) = &mut config.cluster_id {
    if !cluster_id.is_empty() {
      let cluster = get_check_permissions::<Cluster>(
        cluster_id,
        user,
        PermissionLevel::Read.attach(),
      )
      .await
      .with_context(|| {
        format!(
          "Cannot attach Cluster {cluster_id} to this Terraform resource"
        )
      })?;
      *cluster_id = cluster.id;
    }
  }
  Ok(())
}

#[cfg(test)]
mod sanitize_tests {
  use super::super::{KomodoResource, REDACTED};
  use super::*;

  /// Read must not yield `environment`: it is where provider
  /// credentials and `TF_VAR_*` secrets live.
  #[test]
  fn environment_and_webhook_secret_are_redacted() {
    let mut config = TerraformConfig {
      environment: String::from("AWS_SECRET_ACCESS_KEY=abc123"),
      webhook_secret: String::from("s3cret"),
      ..Default::default()
    };
    <Terraform as KomodoResource>::sanitize_config(&mut config);
    assert_eq!(config.environment, REDACTED);
    assert_eq!(config.webhook_secret, REDACTED);
  }

  /// "Not configured" and "configured but hidden" must stay
  /// distinguishable, or debugging a failing run becomes guesswork.
  #[test]
  fn unset_fields_stay_empty() {
    let mut config = TerraformConfig::default();
    <Terraform as KomodoResource>::sanitize_config(&mut config);
    assert!(config.environment.is_empty());
    assert!(config.webhook_secret.is_empty());
  }
}
