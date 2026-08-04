use std::sync::{Arc, OnceLock};

use anyhow::{Context, anyhow};
use async_timing_util::wait_until_timelength;
use database::mungos::find::find_collect;
use futures_util::future::join_all;
use interpolate::Interpolator;
use komodo_client::entities::{
  cluster::{Cluster, ClusterState},
  komodo_timestamp,
};
use mogh_cache::CloneCache;
use periphery_client::api::cluster::{
  ClusterTarget, PollClusterStatus, PollClusterStatusResponse,
};
use tokio::sync::Mutex;

use crate::{
  config::monitoring_interval,
  helpers::{
    periphery_client,
    query::{VariablesAndSecrets, get_variables_and_secrets},
  },
  resource,
  state::{CachedClusterStatus, cluster_status_cache, db_client},
};

const ADDITIONAL_MS: u128 = 1000;

pub fn spawn_cluster_monitoring_loop() {
  tokio::spawn(async move {
    refresh_all_cluster_cache().await;
    let interval = monitoring_interval();
    loop {
      wait_until_timelength(interval, ADDITIONAL_MS).await;
      refresh_all_cluster_cache().await;
    }
  });
}

async fn refresh_all_cluster_cache() {
  let clusters =
    match find_collect(&db_client().clusters, None, None).await {
      Ok(clusters) => clusters,
      Err(e) => {
        error!(
          "Failed to get cluster list (refresh cluster cache) | {e:#}"
        );
        return;
      }
    };
  let futures = clusters.into_iter().map(|cluster| async move {
    refresh_cluster_cache(&cluster, false).await;
  });
  join_all(futures).await;
}

/// Makes sure cache for cluster doesn't update too frequently / simultaneously.
/// If forced, will still block against simultaneous update.
fn refresh_cluster_cache_controller()
-> &'static CloneCache<String, Arc<Mutex<i64>>> {
  static CACHE: OnceLock<CloneCache<String, Arc<Mutex<i64>>>> =
    OnceLock::new();
  CACHE.get_or_init(Default::default)
}

/// The background loop calls this with force: false, which exits early
/// if the lock is busy or it was completed too recently.
pub async fn refresh_cluster_cache(cluster: &Cluster, force: bool) {
  let controller = refresh_cluster_cache_controller()
    .get_or_insert_default(&cluster.id)
    .await;
  let mut lock = match controller.try_lock() {
    Ok(lock) => lock,
    Err(_) if force => controller.lock().await,
    Err(_) => return,
  };

  let now = komodo_timestamp();

  if !force && *lock > now - 1_000 {
    return;
  }

  *lock = now;

  match probe(cluster).await {
    Ok(PollClusterStatusResponse {
      reachable: true, ..
    }) => {
      insert_status(cluster, ClusterState::Ok, None).await;
    }
    // The probe reached Periphery, but kubectl could not reach
    // the api server.
    Ok(PollClusterStatusResponse { err, .. }) => {
      insert_status(
        cluster,
        ClusterState::Unreachable,
        Some(
          anyhow!(
            "{}",
            err.unwrap_or_else(|| String::from("Unknown error"))
          )
          .into(),
        ),
      )
      .await;
    }
    // Could not even ask Periphery.
    Err(e) => {
      insert_status(
        cluster,
        ClusterState::Unreachable,
        Some(e.into()),
      )
      .await;
    }
  }
}

async fn insert_status(
  cluster: &Cluster,
  state: ClusterState,
  err: Option<mogh_error::Serror>,
) {
  cluster_status_cache()
    .insert(
      cluster.id.clone(),
      CachedClusterStatus { state, err }.into(),
    )
    .await;
}

/// The probe always runs on the pinned Server's Periphery, never on Core.
async fn probe(
  cluster: &Cluster,
) -> anyhow::Result<PollClusterStatusResponse> {
  if cluster.config.server_id.is_empty() {
    return Err(anyhow!("No Server attached to this Cluster"));
  }
  let server =
    resource::get::<komodo_client::entities::server::Server>(
      &cluster.config.server_id,
    )
    .await
    .context("Failed to get the Cluster's Server")?;

  // Secrets are interpolated on Core so Periphery never needs to
  // resolve Komodo Variables.
  let mut kubeconfig_contents =
    cluster.config.kubeconfig_contents.clone();
  if !cluster.config.skip_secret_interp
    && !kubeconfig_contents.is_empty()
  {
    let VariablesAndSecrets { variables, secrets } =
      get_variables_and_secrets()
        .await
        .context("Failed to get variables and secrets")?;
    Interpolator::new(Some(&variables), &secrets)
      .interpolate_string(&mut kubeconfig_contents)
      .context("Failed to interpolate variables into kubeconfig")?;
  }

  periphery_client(&server)
    .await?
    // No custom timeout: the transport bounds requests itself now, and
    // an unreachable api server is already capped by the poll's own
    // `kubectl --request-timeout 10s`.
    .request(PollClusterStatus {
      target: ClusterTarget {
        kubeconfig_contents,
        kubeconfig_path: cluster.config.kubeconfig_path.clone(),
        context: cluster.config.context.clone(),
        proxy_url: cluster.config.proxy_url.clone(),
      },
    })
    .await
}
