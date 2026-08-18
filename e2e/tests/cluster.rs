//! Cluster resource: CRUD, toml sync round-trip, and permission checks.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  api::{
    execute::RunSync,
    read::{
      ExportResourcesToToml, GetCluster, ListClusters, ListServers,
    },
    write::{
      CreateCluster, CreateResourceSync, DeleteCluster,
      RenameCluster, UpdateCluster,
    },
  },
  entities::ResourceTarget,
};
use komodo_e2e::{
  authenticated_client, await_update, e2e_env, non_admin_jwt,
  read_as_jwt,
};

/// Names are per-test so tests can run concurrently against one stack.
async fn first_server_id(
  client: &komodo_client::KomodoClient,
) -> String {
  client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id
}

#[tokio::test]
async fn cluster_crud_round_trip() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = first_server_id(&client).await;

  let created = client
    .write(CreateCluster {
      name: "e2e-crud".to_string(),
      config:
        komodo_client::entities::cluster::PartialClusterConfig {
          server_id: Some(server_id.clone()),
          context: Some("kind-komodo-e2e".to_string()),
          namespace: Some("default".to_string()),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to create cluster");

  assert_eq!(created.name, "e2e-crud");
  assert_eq!(created.config.server_id, server_id);
  assert_eq!(created.config.context, "kind-komodo-e2e");

  // Appears in the list with its config surfaced. State is covered by
  // the reachability tests, not here.
  let listed = client
    .read(ListClusters::default())
    .await
    .expect("Failed to list clusters");
  let item = listed
    .iter()
    .find(|c| c.id == created.id)
    .expect("Created cluster missing from ListClusters");
  assert_eq!(item.info.server_id, server_id);
  assert_eq!(item.info.context, "kind-komodo-e2e");

  // Update merges only set fields.
  let updated = client
    .write(UpdateCluster {
      id: created.id.clone(),
      config:
        komodo_client::entities::cluster::PartialClusterConfig {
          namespace: Some("kube-system".to_string()),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to update cluster");
  assert_eq!(updated.config.namespace, "kube-system");
  assert_eq!(
    updated.config.context, "kind-komodo-e2e",
    "Update must not clear fields absent from the partial config"
  );

  client
    .write(RenameCluster {
      id: created.id.clone(),
      name: "e2e-crud-renamed".to_string(),
    })
    .await
    .expect("Failed to rename cluster");

  let fetched = client
    .read(GetCluster {
      cluster: created.id.clone(),
    })
    .await
    .expect("Failed to get cluster after rename");
  assert_eq!(fetched.name, "e2e-crud-renamed");

  client
    .write(DeleteCluster {
      id: created.id.clone(),
    })
    .await
    .expect("Failed to delete cluster");

  assert!(
    client
      .read(GetCluster {
        cluster: created.id.clone()
      })
      .await
      .is_err(),
    "Deleted cluster should no longer be readable"
  );
}

/// Export a Cluster to toml, then sync it back from a fresh sync
/// resource. This is the test that catches a missing Cluster entry in
/// any of the manual toml-sync enumeration sites: those compile fine
/// while silently computing deltas that are never applied.
#[tokio::test]
async fn cluster_toml_sync_round_trip() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = first_server_id(&client).await;

  let created = client
    .write(CreateCluster {
      name: "e2e-toml".to_string(),
      config:
        komodo_client::entities::cluster::PartialClusterConfig {
          server_id: Some(server_id.clone()),
          namespace: Some("original".to_string()),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to create cluster");

  let exported = client
    .read(ExportResourcesToToml {
      targets: vec![ResourceTarget::Cluster(created.id.clone())],
      user_groups: Vec::new(),
      include_variables: false,
      existing: None,
    })
    .await
    .expect("Failed to export cluster toml");

  assert!(
    exported.toml.contains("[[cluster]]"),
    "Export must use the cluster toml header, got:\n{}",
    exported.toml
  );
  assert!(
    exported.toml.contains("namespace = \"original\""),
    "Export must carry config fields, got:\n{}",
    exported.toml
  );

  // Delete it, then let a sync recreate it from the exported toml.
  client
    .write(DeleteCluster {
      id: created.id.clone(),
    })
    .await
    .expect("Failed to delete cluster");

  let sync = client
    .write(CreateResourceSync {
      name: "e2e-toml-sync".to_string(),
      config:
        komodo_client::entities::sync::PartialResourceSyncConfig {
          file_contents: Some(exported.toml.clone()),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to create resource sync");

  let update = client
    .execute(RunSync {
      sync: sync.id.clone(),
      resource_type: None,
      resources: None,
      dry_run: false,
    })
    .await
    .expect("Failed to run sync");
  await_update(&client, &update.id)
    .await
    .expect("Sync update did not succeed");

  let synced = client
    .read(GetCluster {
      cluster: "e2e-toml".to_string(),
    })
    .await
    .expect(
      "Sync did not recreate the Cluster - check that Cluster is \
       registered in every manual toml-sync enumeration site",
    );
  assert_eq!(synced.config.namespace, "original");
  assert_eq!(
    synced.config.server_id, server_id,
    "Sync must resolve the Server name back to its id"
  );

  client
    .write(DeleteCluster { id: synced.id })
    .await
    .expect("Failed to clean up synced cluster");
}

#[tokio::test]
async fn cluster_hidden_from_unpermitted_user() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = first_server_id(&client).await;

  let created = client
    .write(CreateCluster {
      name: "e2e-perms".to_string(),
      config:
        komodo_client::entities::cluster::PartialClusterConfig {
          server_id: Some(server_id),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to create cluster");

  let jwt = non_admin_jwt(&env, "e2e-nobody-cluster")
    .await
    .expect("Failed to create non-admin user");

  // No permission granted: the cluster must be invisible...
  let listed: Vec<serde_json::Value> =
    read_as_jwt(&env, &jwt, "ListClusters", serde_json::json!({}))
      .await
      .expect("ListClusters should succeed but return nothing");
  assert!(
    listed.is_empty(),
    "Non-admin without permission must not see any Cluster, got {listed:?}"
  );

  // ...and fetching it directly must fail.
  let direct = read_as_jwt::<serde_json::Value>(
    &env,
    &jwt,
    "GetCluster",
    serde_json::json!({ "cluster": created.id }),
  )
  .await;
  assert!(
    direct.is_err(),
    "Non-admin without permission must not read a Cluster directly"
  );

  client
    .write(DeleteCluster { id: created.id })
    .await
    .expect("Failed to clean up cluster");
}
