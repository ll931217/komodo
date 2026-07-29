//! Pod logs, including container selection and the Logs permission gate.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    execute::{DeployCluster, DestroyCluster},
    read::{GetClusterPodLog, ListClusterResources, ListServers},
    write::{CreateCluster, DeleteCluster},
  },
  entities::cluster::PartialClusterConfig,
};
use komodo_e2e::require_cluster;
use komodo_e2e::{
  authenticated_client, await_update, e2e_env, non_admin_jwt,
  read_as_jwt,
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

/// Two containers, each printing something distinct, so container
/// selection is observable rather than assumed.
const POD: &str = r#"apiVersion: v1
kind: Pod
metadata:
  name: e2e-log-pod
spec:
  restartPolicy: Never
  containers:
    - name: first
      image: busybox:1.36
      imagePullPolicy: IfNotPresent
      command: ["sh", "-c", "echo hello-from-first; sleep 3600"]
    - name: second
      image: busybox:1.36
      imagePullPolicy: IfNotPresent
      command: ["sh", "-c", "echo hello-from-second; sleep 3600"]
"#;

/// Poll until the pod reports both containers ready, so logs exist.
async fn await_pod_running(client: &KomodoClient, cluster_id: &str) {
  for _ in 0..120 {
    let listing = client
      .read(ListClusterResources {
        cluster: cluster_id.to_string(),
        kind: "pods".to_string(),
        namespace: None,
        all_namespaces: false,
      })
      .await
      .expect("Failed to list pods");
    let running = listing["items"]
      .as_array()
      .into_iter()
      .flatten()
      .find(|item| item["metadata"]["name"] == "e2e-log-pod")
      .map(|pod| pod["status"]["phase"] == "Running")
      .unwrap_or(false);
    if running {
      return;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
  }
  panic!("e2e-log-pod never reached Running");
}

#[tokio::test]
async fn pod_logs_with_container_selection() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig =
    require_cluster!("pod_logs_with_container_selection");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-logs".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig.clone()),
        file_contents: Some(POD.to_string()),
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

  await_pod_running(&client, &cluster.id).await;

  // Each container's log is reachable, and they are distinct.
  for (container, expected) in [
    ("first", "hello-from-first"),
    ("second", "hello-from-second"),
  ] {
    let log = client
      .read(GetClusterPodLog {
        cluster: cluster.id.clone(),
        pod: "e2e-log-pod".to_string(),
        container: Some(container.to_string()),
        namespace: None,
        tail: None,
        previous: false,
      })
      .await
      .expect("Failed to read pod log");
    assert!(
      log.success,
      "Reading the {container} log should succeed: {log:#?}"
    );
    assert!(
      log.stdout.contains(expected),
      "{container} log should contain {expected}, got: {}",
      log.stdout
    );
  }

  // A multi-container pod without a container selection is kubectl's
  // error to report, not a silent wrong answer.
  let log = client
    .read(GetClusterPodLog {
      cluster: cluster.id.clone(),
      pod: "e2e-log-pod".to_string(),
      container: None,
      namespace: None,
      tail: None,
      previous: false,
    })
    .await
    .expect("Request itself should succeed");
  assert!(
    !log.success || log.stdout.contains("hello-from-"),
    "Without a container, kubectl should either error or pick one: {log:#?}"
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

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}

#[tokio::test]
async fn pod_logs_denied_without_permission() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-logs-perms".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let jwt = non_admin_jwt(&env, "e2e-nobody-logs")
    .await
    .expect("Failed to create non-admin user");

  let denied = read_as_jwt::<serde_json::Value>(
    &env,
    &jwt,
    "GetClusterPodLog",
    serde_json::json!({
      "cluster": cluster.id,
      "pod": "anything",
    }),
  )
  .await;
  assert!(
    denied.is_err(),
    "A user without the Logs permission must not read pod logs"
  );

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}
