use std::str::FromStr;

use anyhow::Context;
use database::mungos::{
  find::find_collect,
  mongodb::bson::{doc, oid::ObjectId},
};
use komodo_client::entities::{
  ResourceTarget,
  alert::{Alert, AlertData, SeverityLevel},
  komodo_timestamp,
  terraform::{Terraform, TerraformState},
};

use crate::{alert::send_alerts, state::db_client};

/// Raise or resolve the alert for a Terraform resource after a run.
///
/// Every other resource alerts from the monitor's polling loop. There
/// is nothing to poll here: asking terraform whether reality still
/// matches means running a plan, which is a real execution. So the run
/// itself reports, and a scheduled Procedure running PlanTerraform is
/// what turns that into drift detection.
///
/// No alert buffer either. A buffer exists so one flaky probe cannot
/// page anyone; a run is deliberate and its verdict is not flaky.
pub async fn alert_terraform_state(
  terraform: &Terraform,
  state: TerraformState,
) {
  if !terraform.config.send_alerts {
    return;
  }
  if let Err(e) = alert_terraform_state_inner(terraform, state).await
  {
    error!(
      "Failed to alert on Terraform state | {} | {e:#}",
      terraform.name
    );
  }
}

async fn alert_terraform_state_inner(
  terraform: &Terraform,
  state: TerraformState,
) -> anyhow::Result<()> {
  let open = open_alert(&terraform.id).await?;

  let level = match state {
    // Drift is not a failure: the plan succeeded, and the answer it
    // gave is that reality no longer matches the configuration.
    TerraformState::Drifted => SeverityLevel::Warning,
    TerraformState::Failed => SeverityLevel::Critical,
    TerraformState::Ok => SeverityLevel::Ok,
    // A run that reported nothing is not evidence of health, so it
    // neither opens an alert nor resolves one.
    TerraformState::Unknown => return Ok(()),
  };

  match (level, open) {
    (SeverityLevel::Ok, Some(alert)) => resolve(alert).await,

    (SeverityLevel::Ok, None) => Ok(()),

    // Already open: update it in place rather than re-notifying, unless
    // the severity itself changed (drift that becomes a failed run is
    // news; another drifted plan is not).
    (level, Some(alert)) if alert.level == level => {
      update_in_place(alert, terraform, state).await
    }

    (level, Some(alert)) => {
      resolve(alert).await?;
      open_new(terraform, state, level).await
    }

    (level, None) => open_new(terraform, state, level).await,
  }
}

async fn open_alert(
  terraform_id: &str,
) -> anyhow::Result<Option<Alert>> {
  let alerts = find_collect(
    &db_client().alerts,
    doc! {
      "resolved": false,
      "data.type": "TerraformUnhealthy",
      // AlertData is tagged with the variant under "type" and its
      // fields under "data", so the id is one level deeper than the
      // tag - a nesting a plain "data.id" filter never matches.
      "data.data.id": terraform_id,
    },
    None,
  )
  .await
  .context("Failed to query db for open Terraform alerts")?;
  Ok(alerts.into_iter().next())
}

fn alert_data(
  terraform: &Terraform,
  state: TerraformState,
) -> AlertData {
  AlertData::TerraformUnhealthy {
    id: terraform.id.clone(),
    name: terraform.name.clone(),
    state,
  }
}

async fn open_new(
  terraform: &Terraform,
  state: TerraformState,
  level: SeverityLevel,
) -> anyhow::Result<()> {
  let alert = Alert {
    id: Default::default(),
    ts: komodo_timestamp(),
    resolved: false,
    resolved_ts: None,
    level,
    target: ResourceTarget::Terraform(terraform.id.clone()),
    data: alert_data(terraform, state),
  };
  db_client()
    .alerts
    .insert_one(&alert)
    .await
    .context("Failed to open Terraform alert on db")?;
  send_alerts(&[alert]).await;
  Ok(())
}

async fn update_in_place(
  alert: Alert,
  terraform: &Terraform,
  state: TerraformState,
) -> anyhow::Result<()> {
  let id = ObjectId::from_str(&alert.id)
    .context("Open Terraform alert has an unparseable id")?;
  let data = database::mungos::mongodb::bson::to_bson(&alert_data(
    terraform, state,
  ))
  .context("Failed to serialize Terraform alert data")?;
  db_client()
    .alerts
    .update_one(doc! { "_id": id }, doc! { "$set": { "data": data } })
    .await
    .context("Failed to update Terraform alert on db")?;
  Ok(())
}

async fn resolve(alert: Alert) -> anyhow::Result<()> {
  let id = ObjectId::from_str(&alert.id)
    .context("Open Terraform alert has an unparseable id")?;
  let ts = komodo_timestamp();
  db_client()
    .alerts
    .update_one(
      doc! { "_id": id },
      doc! { "$set": { "resolved": true, "resolved_ts": ts } },
    )
    .await
    .context("Failed to resolve Terraform alert on db")?;

  // Notify with the resolved shape, so the alerter reports recovery.
  let mut alert = alert;
  alert.resolved = true;
  alert.resolved_ts = Some(ts);
  alert.level = SeverityLevel::Ok;
  send_alerts(&[alert]).await;
  Ok(())
}
