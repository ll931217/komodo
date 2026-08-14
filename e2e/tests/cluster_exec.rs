//! Pod exec: an interactive shell into a pod's container, and the
//! Terminal permission that gates it.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    read::{ListClusterResources, ListServers, ListTerminals},
    write::{CreateCluster, DeleteCluster},
  },
  entities::{
    cluster::PartialClusterConfig, terminal::TerminalTarget,
  },
};
use komodo_e2e::require_cluster;
use komodo_e2e::{
  api_credentials, authenticated_client, deploy_manifests, e2e_env,
  execute_terminal, remove_manifests,
};

const POD: &str = r#"apiVersion: v1
kind: Pod
metadata:
  name: e2e-exec-pod
spec:
  restartPolicy: Never
  containers:
    - name: shell
      image: busybox:1.36
      imagePullPolicy: IfNotPresent
      command: ["sh", "-c", "sleep 3600"]
"#;

async fn server_id(client: &KomodoClient) -> String {
  client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id
}

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
      .find(|item| item["metadata"]["name"] == "e2e-exec-pod")
      .map(|pod| pod["status"]["phase"] == "Running")
      .unwrap_or(false);
    if running {
      return;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
  }
  panic!("e2e-exec-pod never reached Running");
}

#[tokio::test]
async fn exec_into_pod_streams_output() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig = require_cluster!("exec_into_pod_streams_output");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-exec".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  // The pod manifest deploys through an Application; the Cluster only
  // supplies the kubeconfig and still owns exec/terminals.
  let application =
    deploy_manifests(&client, &cluster.id, "e2e-exec-pod", POD)
      .await
      .expect("Failed to deploy pod manifest");

  await_pod_running(&client, &cluster.id).await;

  let (key, secret) = api_credentials(&env)
    .await
    .expect("Failed to get api credentials");

  // Runs a command in a shell inside the pod and streams the result.
  //
  // A freshly spawned PTY can echo the command before the shell has
  // produced anything, so this retries until real output appears rather
  // than assuming the first read is complete.
  let mut output = String::new();
  for _ in 0..20 {
    output = execute_terminal(
      &env,
      &key,
      &secret,
      serde_json::json!({
        "target": {
          "type": "ClusterPod",
          "params": {
            "cluster": cluster.id,
            "pod": "e2e-exec-pod",
          }
        },
        "command": "echo exec-works && hostname",
        "init": { "command": "sh" },
      }),
    )
    .await
    .expect("Failed to execute in the pod");
    if output.contains("e2e-exec-pod") {
      break;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
  }

  assert!(
    output.contains("exec-works"),
    "Exec should stream the command output, got: {output}"
  );
  assert!(
    output.contains("e2e-exec-pod"),
    "hostname should be the pod, proving the shell ran inside it, got: {output}"
  );

  // The session shows up as a terminal for this target.
  let terminals = client
    .read(ListTerminals {
      target: Some(TerminalTarget::ClusterPod {
        cluster: cluster.id.clone(),
        namespace: None,
        pod: "e2e-exec-pod".to_string(),
        container: None,
      }),
      use_names: false,
      ..Default::default()
    })
    .await
    .expect("Failed to list terminals");
  assert!(
    !terminals.is_empty(),
    "The exec session should be listed as a terminal"
  );

  remove_manifests(&client, &application)
    .await
    .expect("Failed to clean up Application");

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}
