//! Application resource: CRUD, toml sync round-trip, and permission
//! checks. Deploying is covered in `application_deploy.rs`.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  api::{
    execute::{CancelApplication, RunSync},
    read::{
      ExportResourcesToToml, GetApplication, ListApplications,
      ListServers,
    },
    write::{
      CreateApplication, CreateCluster, CreateResourceSync,
      DeleteApplication, DeleteCluster, RenameApplication,
      UpdateApplication,
    },
  },
  entities::ResourceTarget,
};
use komodo_e2e::{
  authenticated_client, await_update, e2e_env, finished_update,
  non_admin_jwt, read_as_jwt,
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

/// An Application is defined against a Cluster, so every test needs
/// one to point at. It carries no manifests - those live on the
/// Application now.
async fn cluster_id(
  client: &komodo_client::KomodoClient,
  name: &str,
) -> String {
  let server_id = first_server_id(client).await;
  client
    .write(CreateCluster {
      name: name.to_string(),
      config:
        komodo_client::entities::cluster::PartialClusterConfig {
          server_id: Some(server_id),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to create cluster")
    .id
}

#[tokio::test]
async fn application_crud_round_trip() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let cluster = cluster_id(&client, "e2e-app-crud-cluster").await;

  let created = client
    .write(CreateApplication {
      name: "e2e-app-crud".to_string(),
      config:
        komodo_client::entities::application::PartialApplicationConfig {
          cluster_id: Some(cluster.clone()),
          namespace: Some("default".to_string()),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to create application");

  assert_eq!(created.name, "e2e-app-crud");
  assert_eq!(created.config.cluster_id, cluster);
  assert_eq!(created.config.namespace, "default");

  // Appears in the list with its config surfaced. State is covered by
  // the reachability tests, not here.
  let listed = client
    .read(ListApplications::default())
    .await
    .expect("Failed to list applications");
  let item = listed
    .iter()
    .find(|c| c.id == created.id)
    .expect("Created application missing from ListApplications");
  assert_eq!(item.info.cluster_id, cluster);
  assert_eq!(item.info.namespace, "default");

  // Update merges only set fields.
  let updated = client
    .write(UpdateApplication {
      id: created.id.clone(),
      config:
        komodo_client::entities::application::PartialApplicationConfig {
          namespace: Some("kube-system".to_string()),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to update application");
  assert_eq!(updated.config.namespace, "kube-system");
  assert_eq!(
    updated.config.cluster_id, cluster,
    "Update must not clear fields absent from the partial config"
  );

  client
    .write(RenameApplication {
      id: created.id.clone(),
      name: "e2e-app-crud-renamed".to_string(),
    })
    .await
    .expect("Failed to rename application");

  let fetched = client
    .read(GetApplication {
      application: created.id.clone(),
    })
    .await
    .expect("Failed to get application after rename");
  assert_eq!(fetched.name, "e2e-app-crud-renamed");

  client
    .write(DeleteApplication {
      id: created.id.clone(),
    })
    .await
    .expect("Failed to delete application");

  assert!(
    client
      .read(GetApplication {
        application: created.id.clone()
      })
      .await
      .is_err(),
    "Deleted application should no longer be readable"
  );
}

/// Export a Application to toml, then sync it back from a fresh sync
/// resource. This is the test that catches a missing Application entry in
/// any of the manual toml-sync enumeration sites: those compile fine
/// while silently computing deltas that are never applied.
#[tokio::test]
async fn application_toml_sync_round_trip() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let cluster = cluster_id(&client, "e2e-app-toml-cluster").await;

  let created = client
    .write(CreateApplication {
      name: "e2e-app-toml".to_string(),
      config:
        komodo_client::entities::application::PartialApplicationConfig {
          cluster_id: Some(cluster.clone()),
          run_directory: Some("original".to_string()),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to create application");

  let exported = client
    .read(ExportResourcesToToml {
      targets: vec![ResourceTarget::Application(created.id.clone())],
      user_groups: Vec::new(),
      include_variables: false,
      existing: None,
    })
    .await
    .expect("Failed to export application toml");

  assert!(
    exported.toml.contains("[[application]]"),
    "Export must use the application toml header, got:\n{}",
    exported.toml
  );
  assert!(
    exported.toml.contains("run_directory = \"original\""),
    "Export must carry config fields, got:\n{}",
    exported.toml
  );

  // Delete it, then let a sync recreate it from the exported toml.
  client
    .write(DeleteApplication {
      id: created.id.clone(),
    })
    .await
    .expect("Failed to delete application");

  let sync = client
    .write(CreateResourceSync {
      name: "e2e-app-toml-sync".to_string(),
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
    .read(GetApplication {
      application: "e2e-app-toml".to_string(),
    })
    .await
    .expect(
      "Sync did not recreate the Application - check that Application is \
       registered in every manual toml-sync enumeration site",
    );
  assert_eq!(synced.config.run_directory, "original");
  assert_eq!(
    synced.config.cluster_id, cluster,
    "Sync must resolve the Cluster name back to its id"
  );

  client
    .write(DeleteApplication { id: synced.id })
    .await
    .expect("Failed to clean up synced application");
}

#[tokio::test]
async fn application_hidden_from_unpermitted_user() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let cluster = cluster_id(&client, "e2e-app-perms-cluster").await;

  let created = client
    .write(CreateApplication {
      name: "e2e-app-perms".to_string(),
      config:
        komodo_client::entities::application::PartialApplicationConfig {
          cluster_id: Some(cluster.clone()),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to create application");

  let jwt = non_admin_jwt(&env, "e2e-nobody-application")
    .await
    .expect("Failed to create non-admin user");

  // No permission granted: the application must be invisible...
  let listed: Vec<serde_json::Value> = read_as_jwt(
    &env,
    &jwt,
    "ListApplications",
    serde_json::json!({}),
  )
  .await
  .expect("ListApplications should succeed but return nothing");
  assert!(
    listed.is_empty(),
    "Non-admin without permission must not see any Application, got {listed:?}"
  );

  // ...and fetching it directly must fail.
  let direct = read_as_jwt::<serde_json::Value>(
    &env,
    &jwt,
    "GetApplication",
    serde_json::json!({ "application": created.id }),
  )
  .await;
  assert!(
    direct.is_err(),
    "Non-admin without permission must not read a Application directly"
  );

  client
    .write(DeleteApplication { id: created.id })
    .await
    .expect("Failed to clean up application");
  client
    .write(DeleteCluster { id: cluster })
    .await
    .expect("Failed to clean up cluster");
}

/// See the twin in terraform.rs: this proves CancelApplication is
/// wired through every registration site, not that it kills anything.
#[tokio::test]
async fn cancel_application_is_reachable_and_benign_when_idle() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let cluster = cluster_id(&client, "e2e-app-cancel-cluster").await;

  let created = client
    .write(CreateApplication {
      name: "e2e-app-cancel".to_string(),
      config:
        komodo_client::entities::application::PartialApplicationConfig {
          cluster_id: Some(cluster.clone()),
          namespace: Some("default".to_string()),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to create application");

  let update = client
    .execute(CancelApplication {
      application: created.id.clone(),
    })
    .await
    .expect("CancelApplication should be dispatchable");
  let update = finished_update(&client, &update.id)
    .await
    .expect("CancelApplication update should finish");

  assert!(
    update.success,
    "CancelApplication on an idle resource should succeed, got: {update:?}"
  );
  let logs = update
    .logs
    .iter()
    .map(|log| log.stdout.clone())
    .collect::<Vec<_>>()
    .join("\n");
  assert!(
    logs.contains("not currently running"),
    "expected the idle message, got: {logs}"
  );

  client
    .write(DeleteApplication {
      id: created.id.clone(),
    })
    .await
    .ok();
  client.write(DeleteCluster { id: cluster }).await.ok();
}
