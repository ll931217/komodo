use std::{collections::HashMap, str::FromStr, sync::OnceLock};

use anyhow::Context;
use database::mungos::{
  find::find_collect,
  mongodb::bson::{doc, oid::ObjectId, to_bson},
};
use komodo_client::entities::{
  ResourceTarget,
  alert::{Alert, AlertData, AlertDataVariant, SeverityLevel},
  cluster::{Cluster, ClusterState},
  komodo_timestamp,
};

use crate::{
  alert::send_alerts,
  monitor::alert::AlertBuffer,
  state::{cluster_status_cache, db_client},
};

/// Buffer so a single failed probe doesn't page anyone: an alert opens
/// only after two consecutive unreachable polls.
fn alert_buffer() -> &'static AlertBuffer {
  static BUFFER: OnceLock<AlertBuffer> = OnceLock::new();
  BUFFER.get_or_init(AlertBuffer::new)
}

pub async fn alert_clusters(
  ts: i64,
  mut clusters: HashMap<String, Cluster>,
) {
  let open_alerts = match get_open_alerts().await {
    Ok(alerts) => alerts,
    Err(e) => {
      error!("{e:#}");
      return;
    }
  };

  let buffer = alert_buffer();
  let mut to_open = Vec::<Alert>::new();
  let mut to_update = Vec::<Alert>::new();
  let mut to_resolve = Vec::<Alert>::new();

  for (id, status) in cluster_status_cache().get_entries().await {
    let Some(cluster) = clusters.remove(&id) else {
      continue;
    };
    if !cluster.config.send_unreachable_alerts {
      continue;
    }
    let open = open_alerts.get(&id);

    match (status.state, open) {
      // Not probed yet - nothing to say either way.
      (ClusterState::Unknown, _) => {}

      (ClusterState::Unreachable, None) => {
        if buffer.ready_to_open(
          id.clone(),
          AlertDataVariant::ClusterUnreachable,
        ) {
          to_open.push(Alert {
            id: Default::default(),
            ts,
            resolved: false,
            resolved_ts: None,
            level: SeverityLevel::Critical,
            target: ResourceTarget::Cluster(id.clone()),
            data: AlertData::ClusterUnreachable {
              id: id.clone(),
              name: cluster.name.clone(),
              err: status.err.clone(),
            },
          });
        }
      }

      // Still unreachable: keep the alert's error current, but don't
      // re-notify - the severity hasn't changed.
      (ClusterState::Unreachable, Some(alert)) => {
        let mut alert = alert.clone();
        alert.data = AlertData::ClusterUnreachable {
          id: id.clone(),
          name: cluster.name.clone(),
          err: status.err.clone(),
        };
        to_update.push(alert);
      }

      (ClusterState::Ok, Some(alert)) => {
        to_resolve.push(alert.clone());
      }
      (ClusterState::Ok, None) => {
        buffer.reset(id.clone(), AlertDataVariant::ClusterUnreachable)
      }
    }
  }

  tokio::join!(
    open_alerts_on_db(&to_open),
    update_alerts_on_db(&to_update),
    resolve_alerts_on_db(&to_resolve),
  );
}

/// Open, unresolved ClusterUnreachable alerts, keyed by cluster id.
async fn get_open_alerts() -> anyhow::Result<HashMap<String, Alert>> {
  let alerts = find_collect(
    &db_client().alerts,
    doc! {
      "resolved": false,
      "data.type": "ClusterUnreachable",
    },
    None,
  )
  .await
  .context("Failed to query db for open Cluster alerts")?;

  Ok(
    alerts
      .into_iter()
      .filter_map(|alert| {
        let AlertData::ClusterUnreachable { id, .. } = &alert.data
        else {
          return None;
        };
        Some((id.clone(), alert))
      })
      .collect(),
  )
}

async fn open_alerts_on_db(alerts: &[Alert]) {
  if alerts.is_empty() {
    return;
  }
  if let Err(e) = db_client().alerts.insert_many(alerts).await {
    error!("Failed to open Cluster alerts on db | {e:#}");
    return;
  }
  send_alerts(alerts).await;
}

async fn update_alerts_on_db(alerts: &[Alert]) {
  for alert in alerts {
    let Ok(id) = ObjectId::from_str(&alert.id) else {
      continue;
    };
    let Ok(update) = to_bson(alert) else {
      continue;
    };
    if let Err(e) = db_client()
      .alerts
      .update_one(doc! { "_id": id }, doc! { "$set": update })
      .await
    {
      error!("Failed to update Cluster alert on db | {e:#}");
    }
  }
}

async fn resolve_alerts_on_db(alerts: &[Alert]) {
  if alerts.is_empty() {
    return;
  }
  let ts = komodo_timestamp();
  let mut resolved = Vec::with_capacity(alerts.len());
  for alert in alerts {
    let Ok(id) = ObjectId::from_str(&alert.id) else {
      continue;
    };
    if let Err(e) = db_client()
      .alerts
      .update_one(
        doc! { "_id": id },
        doc! { "$set": { "resolved": true, "resolved_ts": ts } },
      )
      .await
    {
      error!("Failed to resolve Cluster alert on db | {e:#}");
      continue;
    }
    // Notify with the resolved shape, so the alerter reports recovery.
    let mut alert = alert.clone();
    alert.resolved = true;
    alert.resolved_ts = Some(ts);
    alert.level = SeverityLevel::Ok;
    resolved.push(alert);
  }
  send_alerts(&resolved).await;
}
