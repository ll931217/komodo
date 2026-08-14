//! Application Deploy / Destroy against the kind cluster, plus the
//! scoping controls the owning Cluster still enforces on them.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    execute::{
      DeployApplication, DestroyApplication, DiffApplication,
    },
    read::{GetApplication, ListServers},
    write::{
      CreateApplication, CreateCluster, DeleteApplication,
      DeleteCluster, UpdateApplication, UpdateCluster,
    },
  },
  entities::{
    application::{ApplicationState, PartialApplicationConfig},
    cluster::PartialClusterConfig,
  },
};
use komodo_e2e::require_cluster;
use komodo_e2e::{
  authenticated_client, await_update, deploy_manifests, e2e_env,
  finished_update, remove_manifests,
};

/// A ConfigMap is enough to prove apply/delete reach the cluster,
/// and needs no image pull.
fn manifests(name: &str) -> String {
  format!(
    "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {name}\ndata:\n  hello: world\n"
  )
}

async fn server_id(client: &KomodoClient) -> String {
  client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id
}

/// Read the ConfigMap straight from the cluster with kubectl, so the
/// assertion does not depend on any Komodo read path.
fn configmap_exists(kubeconfig: &str, name: &str) -> bool {
  std::process::Command::new("kubectl")
    .args([
      "--kubeconfig",
      kubeconfig,
      "get",
      "configmap",
      name,
      "--namespace",
      "default",
    ])
    .output()
    .expect("Failed to run kubectl")
    .status
    .success()
}

#[tokio::test]
async fn deploy_then_destroy_manifests() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig = require_cluster!("deploy_then_destroy_manifests");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-deploy".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig.clone()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let application = client
    .write(CreateApplication {
      name: "e2e-deploy".to_string(),
      config: PartialApplicationConfig {
        cluster_id: Some(cluster.id.clone()),
        file_contents: Some(manifests("e2e-deploy-cm")),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create application");

  let update = client
    .execute(DeployApplication {
      application: application.id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start deploy");
  await_update(&client, &update.id)
    .await
    .expect("Deploy did not succeed");

  assert!(
    configmap_exists(&kubeconfig, "e2e-deploy-cm"),
    "Deploy should have applied the ConfigMap to the cluster"
  );

  // The command output is captured for auditing, not swallowed.
  let logs = client
    .read(komodo_client::api::read::GetUpdate {
      id: update.id.clone(),
    })
    .await
    .expect("Failed to read update")
    .logs;
  assert!(
    logs.iter().any(|log| log.stdout.contains("e2e-deploy-cm")),
    "Update log should carry the kubectl output, got {logs:#?}"
  );

  let update = client
    .execute(DestroyApplication {
      application: application.id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start destroy");
  await_update(&client, &update.id)
    .await
    .expect("Destroy did not succeed");

  assert!(
    !configmap_exists(&kubeconfig, "e2e-deploy-cm"),
    "Destroy should have removed the ConfigMap"
  );

  client
    .write(DeleteApplication { id: application.id })
    .await
    .expect("Failed to clean up application");
  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}

#[tokio::test]
async fn namespace_outside_allow_list_is_rejected() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig =
    require_cluster!("namespace_outside_allow_list_is_rejected");
  let client = authenticated_client(&env).await.unwrap();

  // The allow-list is Cluster policy: the Application below cannot
  // widen it by asking for a different namespace.
  let cluster = client
    .write(CreateCluster {
      name: "e2e-ns-scope".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig.clone()),
        namespaces: Some(vec!["allowed-only".to_string()]),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let application = client
    .write(CreateApplication {
      name: "e2e-ns-scope".to_string(),
      config: PartialApplicationConfig {
        cluster_id: Some(cluster.id.clone()),
        file_contents: Some(manifests("e2e-ns-scope-cm")),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create application");

  let update = client
    .execute(DeployApplication {
      application: application.id.clone(),
      namespace: Some("kube-system".to_string()),
    })
    .await
    .expect("Failed to start deploy");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Deploy update never finished");
  assert!(
    !update.success,
    "Deploy to a disallowed namespace must be refused"
  );
  let logs = format!("{:?}", update.logs);
  assert!(
    logs.contains("kube-system") && logs.contains("allowed"),
    "Update should name the namespace and the allow-list, got: {logs}"
  );

  assert!(
    !configmap_exists(&kubeconfig, "e2e-ns-scope-cm"),
    "Nothing should have reached the cluster"
  );

  client
    .write(DeleteApplication { id: application.id })
    .await
    .expect("Failed to clean up application");
  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}

#[tokio::test]
async fn cluster_scoped_manifests_blocked_when_disabled() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig = require_cluster!(
    "cluster_scoped_manifests_blocked_when_disabled"
  );
  let client = authenticated_client(&env).await.unwrap();

  // Whether cluster-scoped objects are allowed at all is also Cluster
  // policy, checked against the Application's manifests.
  let cluster = client
    .write(CreateCluster {
      name: "e2e-cluster-scope".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig.clone()),
        cluster_resources: Some(false),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let application = client
    .write(CreateApplication {
      name: "e2e-cluster-scope".to_string(),
      config: PartialApplicationConfig {
        cluster_id: Some(cluster.id.clone()),
        // A Namespace is cluster-scoped.
        file_contents: Some(
          "apiVersion: v1\nkind: Namespace\nmetadata:\n  name: e2e-forbidden\n"
            .to_string(),
        ),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create application");

  let update = client
    .execute(DeployApplication {
      application: application.id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start deploy");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Deploy update never finished");
  assert!(
    !update.success,
    "Cluster-scoped manifests must be refused when cluster resources are disabled"
  );
  let logs = format!("{:?}", update.logs);
  assert!(
    logs.contains("Namespace"),
    "Update should name the offending kind, got: {logs}"
  );

  // Flipping the flag on the Cluster lets the same Application's
  // manifests through - the policy lives with the Cluster, not here.
  client
    .write(UpdateCluster {
      id: cluster.id.clone(),
      config: PartialClusterConfig {
        cluster_resources: Some(true),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to update cluster");

  let update = client
    .execute(DeployApplication {
      application: application.id.clone(),
      namespace: None,
    })
    .await
    .expect("Deploy should be allowed once cluster resources are on");
  await_update(&client, &update.id)
    .await
    .expect("Deploy did not succeed");

  let update = client
    .execute(DestroyApplication {
      application: application.id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start destroy");
  await_update(&client, &update.id)
    .await
    .expect("Destroy did not succeed");

  client
    .write(DeleteApplication { id: application.id })
    .await
    .expect("Failed to clean up application");
  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}

#[tokio::test]
async fn diff_reports_pending_change_without_applying_it() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig = require_cluster!(
    "diff_reports_pending_change_without_applying_it"
  );
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-diff".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig.clone()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  // Deploy first, so there is a live object to diff against - this
  // test only cares about the diff, not the deploy that set it up.
  let application_id = deploy_manifests(
    &client,
    &cluster.id,
    "e2e-diff",
    &manifests("e2e-diff-cm"),
  )
  .await
  .expect("Failed to deploy manifests");

  // In sync: diff succeeds and reports nothing pending.
  let update = client
    .execute(DiffApplication {
      application: application_id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start diff");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Diff update never finished");
  assert!(
    update.success,
    "Diff on an in-sync Application should succeed, got {:#?}",
    update.logs
  );
  let in_sync_output: String =
    update.logs.iter().map(|log| log.stdout.clone()).collect();
  assert!(
    !in_sync_output.contains("hello"),
    "In-sync diff should report no changes, got: {in_sync_output}"
  );

  // Change the manifest, then diff again.
  client
    .write(UpdateApplication {
      id: application_id.clone(),
      config: PartialApplicationConfig {
        file_contents: Some(
          "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: e2e-diff-cm\ndata:\n  hello: changed\n"
            .to_string(),
        ),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to update application");

  let update = client
    .execute(DiffApplication {
      application: application_id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start diff");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Diff update never finished");
  assert!(
    update.success,
    "Diff finding changes is a success, not a failure: {:#?}",
    update.logs
  );
  let output: String =
    update.logs.iter().map(|log| log.stdout.clone()).collect();
  assert!(
    output.contains("changed"),
    "Diff should show the pending change, got: {output}"
  );

  // And the cluster itself is untouched by the diff.
  let live = std::process::Command::new("kubectl")
    .args([
      "--kubeconfig",
      &kubeconfig,
      "get",
      "configmap",
      "e2e-diff-cm",
      "--namespace",
      "default",
      "-o",
      "jsonpath={.data.hello}",
    ])
    .output()
    .expect("Failed to run kubectl");
  assert_eq!(
    String::from_utf8_lossy(&live.stdout),
    "world",
    "Diff must not change the live object"
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
async fn diff_state_reflects_drift() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig = require_cluster!("diff_state_reflects_drift");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-diff-state".to_string(),
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
    "e2e-diff-state",
    &manifests("e2e-diff-state-cm"),
  )
  .await
  .expect("Failed to deploy manifests");

  let state = client
    .read(GetApplication {
      application: application_id.clone(),
    })
    .await
    .expect("Failed to read application")
    .info
    .state;
  assert_eq!(
    state,
    ApplicationState::Deployed,
    "A successful Deploy should leave the Application Deployed"
  );

  // An in-sync Diff is a success, and leaves the state Deployed
  // rather than claiming something new was measured.
  let update = client
    .execute(DiffApplication {
      application: application_id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start diff");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Diff update never finished");
  assert!(
    update.success,
    "Diff against an unchanged deployment should report success, got {:#?}",
    update.logs
  );
  let state = client
    .read(GetApplication {
      application: application_id.clone(),
    })
    .await
    .expect("Failed to read application")
    .info
    .state;
  assert_eq!(
    state,
    ApplicationState::Deployed,
    "A Diff finding nothing pending should leave the Application Deployed"
  );

  // Drift the manifest, then diff again: still a success, but now
  // Drifted rather than Deployed.
  client
    .write(UpdateApplication {
      id: application_id.clone(),
      config: PartialApplicationConfig {
        file_contents: Some(
          "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: e2e-diff-state-cm\ndata:\n  hello: changed\n"
            .to_string(),
        ),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to update application");

  let update = client
    .execute(DiffApplication {
      application: application_id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start diff");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Diff update never finished");
  assert!(
    update.success,
    "A Diff finding changes is still a success, not a failure: {:#?}",
    update.logs
  );
  let state = client
    .read(GetApplication {
      application: application_id.clone(),
    })
    .await
    .expect("Failed to read application")
    .info
    .state;
  assert_eq!(
    state,
    ApplicationState::Drifted,
    "A Diff finding pending changes should mark the Application Drifted"
  );

  remove_manifests(&client, &application_id)
    .await
    .expect("Failed to clean up application");
  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}
