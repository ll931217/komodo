//! Unreachable alerts: raised when a Cluster stops answering, resolved
//! when it answers again.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    read::{ListAlerts, ListServers},
    write::{CreateCluster, DeleteCluster, UpdateCluster},
  },
  entities::{
    alert::{Alert, SeverityLevel},
    cluster::PartialClusterConfig,
  },
};
use komodo_e2e::{authenticated_client, e2e_env, kubeconfig};

async fn server_id(client: &KomodoClient) -> String {
  client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id
}

/// Poll alerts for this Cluster until one matches, or give up.
///
/// The alert only opens after two consecutive failed probes (a buffer
/// against transient flaps), so this waits rather than checking once.
async fn await_alert(
  client: &KomodoClient,
  cluster_id: &str,
  want_resolved: bool,
) -> Option<Alert> {
  for _ in 0..60 {
    let alerts = client
      .read(ListAlerts {
        query: Some(bson::doc! {
          "data.type": "ClusterUnreachable",
          "data.data.id": cluster_id,
          "resolved": want_resolved,
        }),
        page: 0,
      })
      .await
      .expect("Failed to list alerts")
      .alerts;
    if let Some(alert) = alerts.into_iter().next() {
      return Some(alert);
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
  }
  None
}

#[tokio::test]
async fn unreachable_cluster_raises_alert() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  // A kubeconfig that cannot work: the probe fails, no cluster needed.
  let cluster = client
    .write(CreateCluster {
      name: "e2e-alert".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(
          "/nonexistent/kubeconfig-for-alerting".to_string(),
        ),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let alert = await_alert(&client, &cluster.id, false)
    .await
    .expect("An unreachable Cluster should raise an alert");
  assert!(
    matches!(alert.level, SeverityLevel::Critical),
    "Unreachable should be Critical, got {:?}",
    alert.level
  );
  assert!(
    !alert.resolved,
    "A cluster that is still unreachable should have an open alert"
  );

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}

#[tokio::test]
async fn alert_resolves_when_cluster_recovers() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  // Recovery needs a kubeconfig that actually works.
  let Some(good_kubeconfig) = kubeconfig() else {
    eprintln!(
      "SKIP alert_resolves_when_cluster_recovers: no kind cluster available"
    );
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-alert-recover".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(
          "/nonexistent/kubeconfig-recover".to_string(),
        ),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  await_alert(&client, &cluster.id, false)
    .await
    .expect("Expected the unreachable alert to open first");

  // Point it at the real cluster: the next poll should resolve.
  client
    .write(UpdateCluster {
      id: cluster.id.clone(),
      config: PartialClusterConfig {
        kubeconfig_path: Some(good_kubeconfig),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to update cluster");

  let resolved = await_alert(&client, &cluster.id, true)
    .await
    .expect("Recovering should resolve the open alert");
  assert!(
    resolved.resolved,
    "The alert should be marked resolved once reachable again"
  );

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}
