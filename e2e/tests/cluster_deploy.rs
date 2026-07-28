//! Cluster Deploy / Destroy against the kind cluster, plus the
//! scoping controls that gate them.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    execute::{DeployCluster, DestroyCluster},
    read::ListServers,
    write::{CreateCluster, DeleteCluster, UpdateCluster},
  },
  entities::cluster::PartialClusterConfig,
};
use komodo_e2e::{
  authenticated_client, await_update, e2e_env, finished_update,
};

/// A ConfigMap is enough to prove apply/delete reach the cluster,
/// and needs no image pull.
fn manifests(name: &str) -> String {
  format!(
    "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {name}\ndata:\n  hello: world\n"
  )
}

fn kubeconfig() -> String {
  std::env::var("KOMODO_E2E_KUBECONFIG")
    .expect("KOMODO_E2E_KUBECONFIG must be set by scripts/e2e.sh")
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
fn configmap_exists(name: &str) -> bool {
  std::process::Command::new("kubectl")
    .args([
      "--kubeconfig",
      &kubeconfig(),
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
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-deploy".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig()),
        file_contents: Some(manifests("e2e-deploy-cm")),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let update = client
    .execute(DeployCluster {
      cluster: cluster.id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start deploy");
  await_update(&client, &update.id)
    .await
    .expect("Deploy did not succeed");

  assert!(
    configmap_exists("e2e-deploy-cm"),
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
    .execute(DestroyCluster {
      cluster: cluster.id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start destroy");
  await_update(&client, &update.id)
    .await
    .expect("Destroy did not succeed");

  assert!(
    !configmap_exists("e2e-deploy-cm"),
    "Destroy should have removed the ConfigMap"
  );

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
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-ns-scope".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig()),
        file_contents: Some(manifests("e2e-ns-scope-cm")),
        namespaces: Some(vec!["allowed-only".to_string()]),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let update = client
    .execute(DeployCluster {
      cluster: cluster.id.clone(),
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
    !configmap_exists("e2e-ns-scope-cm"),
    "Nothing should have reached the cluster"
  );

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
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-cluster-scope".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig()),
        // A Namespace is cluster-scoped.
        file_contents: Some(
          "apiVersion: v1\nkind: Namespace\nmetadata:\n  name: e2e-forbidden\n"
            .to_string(),
        ),
        cluster_resources: Some(false),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let update = client
    .execute(DeployCluster {
      cluster: cluster.id.clone(),
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

  // Flipping the flag on lets the same manifests through.
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
    .execute(DeployCluster {
      cluster: cluster.id.clone(),
      namespace: None,
    })
    .await
    .expect("Deploy should be allowed once cluster resources are on");
  await_update(&client, &update.id)
    .await
    .expect("Deploy did not succeed");

  let update = client
    .execute(DestroyCluster {
      cluster: cluster.id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start destroy");
  await_update(&client, &update.id)
    .await
    .expect("Destroy did not succeed");

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}
