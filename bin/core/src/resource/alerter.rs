use database::mungos::mongodb::Collection;
use komodo_client::entities::{
  Operation, ResourceTarget, ResourceTargetVariant,
  alerter::{
    Alerter, AlerterConfig, AlerterConfigDiff, AlerterEndpoint,
    AlerterListItem, AlerterListItemInfo, AlerterQuerySpecifics,
    PartialAlerterConfig,
  },
  resource::Resource,
  update::Update,
  user::User,
};

use crate::state::db_client;

impl super::KomodoResource for Alerter {
  type Config = AlerterConfig;
  type PartialConfig = PartialAlerterConfig;
  type ConfigDiff = AlerterConfigDiff;
  type Info = ();
  type ListItem = AlerterListItem;
  type QuerySpecifics = AlerterQuerySpecifics;

  fn resource_type() -> ResourceTargetVariant {
    ResourceTargetVariant::Alerter
  }

  fn resource_target(id: impl Into<String>) -> ResourceTarget {
    ResourceTarget::Alerter(id.into())
  }

  /// Every endpoint variant routes through a URL, and for Slack, Discord
  /// and Pushover that URL *is* the credential — anyone holding it can
  /// post into the channel from outside Komodo entirely.
  ///
  /// Custom and Ntfy URLs are redacted too: they are just as likely to
  /// carry a token in the path or query, and a caller who cannot be
  /// trusted with the others should not get to enumerate internal
  /// endpoints either. Matched exhaustively so a new variant has to make
  /// this decision rather than defaulting to exposed.
  fn sanitize_config(config: &mut Self::Config) {
    let url = match &mut config.endpoint {
      AlerterEndpoint::Custom(endpoint) => &mut endpoint.url,
      AlerterEndpoint::Slack(endpoint) => &mut endpoint.url,
      AlerterEndpoint::Discord(endpoint) => &mut endpoint.url,
      AlerterEndpoint::Ntfy(endpoint) => &mut endpoint.url,
      AlerterEndpoint::Pushover(endpoint) => &mut endpoint.url,
    };
    *url = super::redacted(url);
  }

  fn coll() -> &'static Collection<Resource<Self::Config, Self::Info>>
  {
    &db_client().alerters
  }

  async fn to_list_item(
    alerter: Resource<Self::Config, Self::Info>,
  ) -> Self::ListItem {
    AlerterListItem {
      name: alerter.name,
      id: alerter.id,
      template: alerter.template,
      tags: alerter.tags,
      resource_type: ResourceTargetVariant::Alerter,
      info: AlerterListItemInfo {
        endpoint_type: alerter.config.endpoint.into(),
        enabled: alerter.config.enabled,
      },
    }
  }

  async fn busy(_id: &String) -> anyhow::Result<bool> {
    Ok(false)
  }

  // CREATE

  fn create_operation() -> Operation {
    Operation::CreateAlerter
  }

  fn user_can_create(user: &User) -> bool {
    user.admin
  }

  async fn validate_create_config(
    _config: &mut Self::PartialConfig,
    _user: &User,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  async fn post_create(
    _created: &Resource<Self::Config, Self::Info>,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  // UPDATE

  fn update_operation() -> Operation {
    Operation::UpdateAlerter
  }

  async fn validate_update_config(
    _id: &str,
    _config: &mut Self::PartialConfig,
    _user: &User,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  async fn post_update(
    _updated: &Self,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  // RENAME

  fn rename_operation() -> Operation {
    Operation::RenameAlerter
  }

  // DELETE

  fn delete_operation() -> Operation {
    Operation::DeleteAlerter
  }

  async fn pre_delete(
    _resource: &Resource<Self::Config, Self::Info>,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }

  async fn post_delete(
    _resource: &Resource<Self::Config, Self::Info>,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    Ok(())
  }
}
