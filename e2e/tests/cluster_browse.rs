//! Browsing cluster objects as opaque JSON, and deleting one.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    execute::DeleteClusterObject,
    read::{
      InspectClusterResource, ListClusterResources, ListServers,
    },
    write::{CreateCluster, DeleteCluster},
  },
  entities::cluster::PartialClusterConfig,
};
use komodo_e2e::require_cluster;
use komodo_e2e::{
  authenticated_client, await_update, deploy_manifests, e2e_env,
  finished_update, remove_manifests,
};

async fn server_id(client: &KomodoClient) -> String {
  client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id
}

/// Names in the `items` array of a `kubectl get -o json` collection.
fn item_names(listing: &serde_json::Value) -> Vec<String> {
  listing["items"]
    .as_array()
    .expect("Listing should have an items array")
    .iter()
    .filter_map(|item| {
      item["metadata"]["name"].as_str().map(str::to_string)
    })
    .collect()
}

#[tokio::test]
async fn list_and_inspect_deployed_objects() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig =
    require_cluster!("list_and_inspect_deployed_objects");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-browse".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig.clone()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let application_id = deploy_manifests(
    &client,
    &cluster.id,
    "e2e-browse",
    "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: e2e-browse-cm\ndata:\n  hello: world\n",
  )
  .await
  .expect("Failed to deploy manifests");

  // The applied object shows up in the listing.
  let listing = client
    .read(ListClusterResources {
      cluster: cluster.id.clone(),
      kind: "configmaps".to_string(),
      namespace: None,
      all_namespaces: false,
    })
    .await
    .expect("Failed to list configmaps");
  assert!(
    item_names(&listing).contains(&"e2e-browse-cm".to_string()),
    "Listing should include the deployed ConfigMap, got {:?}",
    item_names(&listing)
  );

  // And opens as JSON on its own.
  let object = client
    .read(InspectClusterResource {
      cluster: cluster.id.clone(),
      kind: "configmap".to_string(),
      name: "e2e-browse-cm".to_string(),
      namespace: None,
    })
    .await
    .expect("Failed to inspect configmap");
  assert_eq!(object["kind"], "ConfigMap");
  assert_eq!(object["data"]["hello"], "world");

  // Kind selector actually scopes the read: pods, not configmaps.
  let pods = client
    .read(ListClusterResources {
      cluster: cluster.id.clone(),
      kind: "pods".to_string(),
      namespace: None,
      all_namespaces: false,
    })
    .await
    .expect("Failed to list pods");
  assert!(
    !item_names(&pods).contains(&"e2e-browse-cm".to_string()),
    "Pods listing must not contain a ConfigMap"
  );

  remove_manifests(&client, &application_id)
    .await
    .expect("Failed to clean up application");

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}

#[tokio::test]
async fn delete_object_removes_it() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig = require_cluster!("delete_object_removes_it");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-delete-object".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig.clone()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let application_id = deploy_manifests(
    &client,
    &cluster.id,
    "e2e-delete-object",
    "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: e2e-delete-me\ndata:\n  hello: world\n",
  )
  .await
  .expect("Failed to deploy manifests");

  let update = client
    .execute(DeleteClusterObject {
      cluster: cluster.id.clone(),
      kind: "configmap".to_string(),
      name: "e2e-delete-me".to_string(),
      namespace: None,
    })
    .await
    .expect("Failed to start delete");
  await_update(&client, &update.id)
    .await
    .expect("Delete did not succeed");

  let listing = client
    .read(ListClusterResources {
      cluster: cluster.id.clone(),
      kind: "configmaps".to_string(),
      namespace: None,
      all_namespaces: false,
    })
    .await
    .expect("Failed to list configmaps");
  assert!(
    !item_names(&listing).contains(&"e2e-delete-me".to_string()),
    "Deleted object should be gone, got {:?}",
    item_names(&listing)
  );

  remove_manifests(&client, &application_id)
    .await
    .expect("Failed to clean up application");

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}

/// The cluster-resources flag gates reads and deletes, not just applies.
#[tokio::test]
async fn cluster_scoped_reads_blocked_when_disabled() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig =
    require_cluster!("cluster_scoped_reads_blocked_when_disabled");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-browse-scope".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig.clone()),
        cluster_resources: Some(false),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let err = client
    .read(ListClusterResources {
      cluster: cluster.id.clone(),
      kind: "namespaces".to_string(),
      namespace: None,
      all_namespaces: false,
    })
    .await
    .expect_err("Listing a cluster-scoped kind must be refused");
  let err = format!("{err:#}");
  assert!(
    err.contains("cluster-scoped"),
    "Error should explain the refusal, got: {err}"
  );

  // Namespaced kinds still work on the same Cluster.
  client
    .read(ListClusterResources {
      cluster: cluster.id.clone(),
      kind: "configmaps".to_string(),
      namespace: None,
      all_namespaces: false,
    })
    .await
    .expect("Namespaced kinds should still be readable");

  // Deleting a cluster-scoped object is refused too.
  let update = client
    .execute(DeleteClusterObject {
      cluster: cluster.id.clone(),
      kind: "namespace".to_string(),
      name: "kube-public".to_string(),
      namespace: None,
    })
    .await
    .expect("Failed to start delete");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Delete update never finished");
  assert!(
    !update.success,
    "Deleting a cluster-scoped object must be refused"
  );

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}
