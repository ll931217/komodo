use std::str::FromStr;

use anyhow::Context;
use database::mungos::{
  find::find_collect,
  mongodb::bson::{doc, oid::ObjectId},
};
use komodo_client::entities::{
  ResourceTarget,
  alert::{Alert, AlertData, SeverityLevel},
  application::{Application, ApplicationState},
  komodo_timestamp,
};

use crate::{alert::send_alerts, state::db_client};

/// Raise or resolve the alert for an Application after an execution.
///
/// Every other resource alerts from the monitor's polling loop. There
/// is nothing to poll here: asking whether the cluster still matches
/// the manifests means running `kubectl diff`, which is a real call to
/// the api server. So the execution itself reports, and a scheduled
/// Procedure running DiffApplication is what turns that into drift
/// detection.
///
/// No alert buffer either. A buffer exists so one flaky probe cannot
/// page anyone; a run is deliberate and its verdict is not flaky.
pub async fn alert_application_state(
  application: &Application,
  state: ApplicationState,
) {
  if !application.config.send_alerts {
    return;
  }
  if let Err(e) =
    alert_application_state_inner(application, state).await
  {
    error!(
      "Failed to alert on Application state | {} | {e:#}",
      application.name
    );
  }
}

async fn alert_application_state_inner(
  application: &Application,
  state: ApplicationState,
) -> anyhow::Result<()> {
  let open = open_alert(&application.id).await?;

  let level = match state {
    // Drift is not a failure: the diff succeeded, and the answer it
    // gave is that the cluster no longer matches the manifests.
    ApplicationState::Drifted => SeverityLevel::Warning,
    ApplicationState::Failed => SeverityLevel::Critical,
    ApplicationState::Deployed => SeverityLevel::Ok,
    // Never deployed, or destroyed since. Neither is evidence of
    // health, so it neither opens an alert nor resolves one.
    ApplicationState::Unknown => return Ok(()),
  };

  match (level, open) {
    (SeverityLevel::Ok, Some(alert)) => resolve(alert).await,

    (SeverityLevel::Ok, None) => Ok(()),

    // Already open: update it in place rather than re-notifying, unless
    // the severity itself changed (drift that becomes a failed run is
    // news; another drifted plan is not).
    (level, Some(alert)) if alert.level == level => {
      update_in_place(alert, application, state).await
    }

    (level, Some(alert)) => {
      resolve(alert).await?;
      open_new(application, state, level).await
    }

    (level, None) => open_new(application, state, level).await,
  }
}

async fn open_alert(
  application_id: &str,
) -> anyhow::Result<Option<Alert>> {
  let alerts = find_collect(
    &db_client().alerts,
    doc! {
      "resolved": false,
      "data.type": "ApplicationUnhealthy",
      // AlertData is tagged with the variant under "type" and its
      // fields under "data", so the id is one level deeper than the
      // tag - a nesting a plain "data.id" filter never matches.
      "data.data.id": application_id,
    },
    None,
  )
  .await
  .context("Failed to query db for open Application alerts")?;
  Ok(alerts.into_iter().next())
}

fn alert_data(
  application: &Application,
  state: ApplicationState,
) -> AlertData {
  AlertData::ApplicationUnhealthy {
    id: application.id.clone(),
    name: application.name.clone(),
    state,
  }
}

async fn open_new(
  application: &Application,
  state: ApplicationState,
  level: SeverityLevel,
) -> anyhow::Result<()> {
  let alert = Alert {
    id: Default::default(),
    ts: komodo_timestamp(),
    resolved: false,
    resolved_ts: None,
    level,
    target: ResourceTarget::Application(application.id.clone()),
    data: alert_data(application, state),
  };
  db_client()
    .alerts
    .insert_one(&alert)
    .await
    .context("Failed to open Application alert on db")?;
  send_alerts(&[alert]).await;
  Ok(())
}

async fn update_in_place(
  alert: Alert,
  application: &Application,
  state: ApplicationState,
) -> anyhow::Result<()> {
  let id = ObjectId::from_str(&alert.id)
    .context("Open Application alert has an unparseable id")?;
  let data = database::mungos::mongodb::bson::to_bson(&alert_data(
    application,
    state,
  ))
  .context("Failed to serialize Application alert data")?;
  db_client()
    .alerts
    .update_one(doc! { "_id": id }, doc! { "$set": { "data": data } })
    .await
    .context("Failed to update Application alert on db")?;
  Ok(())
}

async fn resolve(alert: Alert) -> anyhow::Result<()> {
  let id = ObjectId::from_str(&alert.id)
    .context("Open Application alert has an unparseable id")?;
  let ts = komodo_timestamp();
  db_client()
    .alerts
    .update_one(
      doc! { "_id": id },
      doc! { "$set": { "resolved": true, "resolved_ts": ts } },
    )
    .await
    .context("Failed to resolve Application alert on db")?;

  // Notify with the resolved shape, so the alerter reports recovery.
  let mut alert = alert;
  alert.resolved = true;
  alert.resolved_ts = Some(ts);
  alert.level = SeverityLevel::Ok;
  send_alerts(&[alert]).await;
  Ok(())
}
