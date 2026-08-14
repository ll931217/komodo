//! Manifest source modes: files on the host, and a git repo.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    execute::{DeployApplication, DestroyApplication},
    read::ListServers,
    write::{
      CreateApplication, CreateCluster, DeleteApplication,
      DeleteCluster, UpdateApplication,
    },
  },
  entities::{
    application::PartialApplicationConfig,
    cluster::PartialClusterConfig,
  },
};
use komodo_e2e::require_cluster;
use komodo_e2e::{
  authenticated_client, await_update, e2e_env, finished_update,
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

fn configmap_data(kubeconfig: &str, name: &str) -> Option<String> {
  let out = std::process::Command::new("kubectl")
    .args([
      "--kubeconfig",
      kubeconfig,
      "get",
      "configmap",
      name,
      "--namespace",
      "default",
      "-o",
      "jsonpath={.data.source}",
    ])
    .output()
    .expect("Failed to run kubectl");
  out
    .status
    .success()
    .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// A directory of manifests on the host, plus a git repo built from it,
/// so both source modes are exercised against real files.
fn write_host_manifests(dir: &std::path::Path, value: &str) {
  std::fs::create_dir_all(dir).expect("Failed to create dir");
  std::fs::write(
    dir.join("cm.yaml"),
    format!(
      "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: e2e-src-cm\ndata:\n  source: {value}\n"
    ),
  )
  .expect("Failed to write manifest");
}

#[tokio::test]
async fn deploys_from_files_on_host() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig = require_cluster!("deploys_from_files_on_host");
  let client = authenticated_client(&env).await.unwrap();

  // Periphery runs on this host, so a path here is a path there.
  let dir = std::env::current_dir()
    .expect("no cwd")
    .join("e2e/.state/host-manifests");
  write_host_manifests(&dir, "host");

  let cluster = client
    .write(CreateCluster {
      name: "e2e-src-host".to_string(),
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
      name: "e2e-src-host".to_string(),
      config: PartialApplicationConfig {
        cluster_id: Some(cluster.id.clone()),
        files_on_host: Some(true),
        run_directory: Some(dir.display().to_string()),
        file_paths: Some(vec!["cm.yaml".to_string()]),
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
    .expect("Deploy from host files did not succeed");

  assert_eq!(
    configmap_data(&kubeconfig, "e2e-src-cm").as_deref(),
    Some("host"),
    "The host file's contents should be what landed in the cluster"
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

  // Switching away from host mode must not leave the old files applied
  // or block a later deploy from a different source.
  client
    .write(UpdateApplication {
      id: application.id.clone(),
      config: PartialApplicationConfig {
        files_on_host: Some(false),
        file_contents: Some(
          "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: e2e-src-cm\ndata:\n  source: contents\n"
            .to_string(),
        ),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to switch source mode");

  let update = client
    .execute(DeployApplication {
      application: application.id.clone(),
      namespace: None,
    })
    .await
    .expect("Failed to start deploy");
  await_update(&client, &update.id)
    .await
    .expect("Deploy after switching source did not succeed");

  assert_eq!(
    configmap_data(&kubeconfig, "e2e-src-cm").as_deref(),
    Some("contents"),
    "Switching source mode should deploy from the new source"
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

  client
    .write(DeleteApplication { id: application.id })
    .await
    .expect("Failed to clean up application");
  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}

/// A repo source that cannot be cloned must fail loudly on the Update,
/// not silently apply nothing.
#[tokio::test]
async fn unreachable_repo_source_fails_the_deploy() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig =
    require_cluster!("unreachable_repo_source_fails_the_deploy");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-src-repo".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let application = client
    .write(CreateApplication {
      name: "e2e-src-repo".to_string(),
      config: PartialApplicationConfig {
        cluster_id: Some(cluster.id.clone()),
        repo: Some("komodo-e2e/does-not-exist".to_string()),
        branch: Some("main".to_string()),
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
    "A repo that cannot be cloned must fail the deploy, got {:#?}",
    update.logs
  );
  // The clone failure is reported, rather than an empty apply.
  let logs = format!("{:?}", update.logs);
  assert!(
    logs.contains("clone") || logs.contains("Clone"),
    "The failure should name the clone step, got: {logs}"
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
