//! Cluster reachability: the probe runs on the pinned Server's
//! Periphery and drives the Cluster's state without user action.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    read::{ListClusters, ListServers},
    write::{CreateCluster, DeleteCluster},
  },
  entities::cluster::{ClusterState, PartialClusterConfig},
};
use komodo_e2e::{authenticated_client, e2e_env};

/// The harness writes the kind cluster's kubeconfig here and Periphery
/// runs on the same host, so the path resolves for both.
fn kind_kubeconfig() -> String {
  std::env::var("KOMODO_E2E_KUBECONFIG").unwrap_or_else(|_| {
    format!(
      "{}/e2e/.state/kubeconfig",
      std::env::var("KOMODO_E2E_REPO_DIR")
        .unwrap_or_else(|_| ".".to_string())
    )
  })
}

/// Poll the Cluster's list state until it leaves Unknown.
///
/// State is produced by a background loop, so the test waits for the
/// loop rather than triggering it - that is the behaviour under test.
async fn await_state(
  client: &KomodoClient,
  cluster_id: &str,
) -> ClusterState {
  for _ in 0..60 {
    let state = client
      .read(ListClusters::default())
      .await
      .expect("Failed to list clusters")
      .into_iter()
      .find(|c| c.id == cluster_id)
      .map(|c| c.info.state);
    match state {
      Some(ClusterState::Unknown) | None => {}
      Some(state) => return state,
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
  }
  panic!("Cluster {cluster_id} stayed Unknown for 30s");
}

async fn create_cluster(
  client: &KomodoClient,
  name: &str,
  config: PartialClusterConfig,
) -> String {
  client
    .write(CreateCluster {
      name: name.to_string(),
      config,
    })
    .await
    .expect("Failed to create cluster")
    .id
}

#[tokio::test]
async fn reachable_cluster_reports_ok() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id;

  let id = create_cluster(
    &client,
    "e2e-reachable",
    PartialClusterConfig {
      server_id: Some(server_id),
      kubeconfig_path: Some(kind_kubeconfig()),
      ..Default::default()
    },
  )
  .await;

  let state = await_state(&client, &id).await;
  assert!(
    matches!(state, ClusterState::Ok),
    "Expected Ok against the kind cluster, got {state:?}"
  );

  client
    .write(DeleteCluster { id })
    .await
    .expect("Failed to clean up cluster");
}

#[tokio::test]
async fn broken_kubeconfig_reports_unreachable() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id;

  let id = create_cluster(
    &client,
    "e2e-unreachable",
    PartialClusterConfig {
      server_id: Some(server_id),
      kubeconfig_path: Some(
        "/nonexistent/kubeconfig-does-not-exist".to_string(),
      ),
      ..Default::default()
    },
  )
  .await;

  let state = await_state(&client, &id).await;
  assert!(
    matches!(state, ClusterState::Unreachable),
    "Expected Unreachable for a missing kubeconfig, got {state:?}"
  );

  // The failure reason is surfaced, not swallowed.
  let err = client
    .read(ListClusters::default())
    .await
    .expect("Failed to list clusters")
    .into_iter()
    .find(|c| c.id == id)
    .and_then(|c| c.info.err);
  assert!(
    err.is_some(),
    "Unreachable Cluster should carry the probe error"
  );

  client
    .write(DeleteCluster { id })
    .await
    .expect("Failed to clean up cluster");
}
