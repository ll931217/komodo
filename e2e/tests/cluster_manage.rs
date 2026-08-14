//! Management verbs on live objects: rollout restart, scale, single
//! object apply, and the workload-kind gate.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    execute::{
      ApplyClusterObject, DeployApplication, RestartClusterWorkload,
      ScaleClusterWorkload,
    },
    read::{ListClusterResources, ListServers},
    write::{CreateApplication, CreateCluster, DeleteCluster},
  },
  entities::{
    application::PartialApplicationConfig,
    cluster::PartialClusterConfig,
  },
};
use komodo_e2e::require_cluster;
use komodo_e2e::{
  authenticated_client, await_update, deploy_manifests, e2e_env,
  finished_update, remove_manifests,
};

const DEPLOYMENT: &str = r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: e2e-manage-web
spec:
  replicas: 1
  selector:
    matchLabels:
      app: e2e-manage-web
  template:
    metadata:
      labels:
        app: e2e-manage-web
    spec:
      containers:
        - name: web
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

async fn deployment_json(
  client: &KomodoClient,
  cluster: &str,
) -> serde_json::Value {
  client
    .read(ListClusterResources {
      cluster: cluster.to_string(),
      kind: "deployments".to_string(),
      namespace: None,
      all_namespaces: false,
    })
    .await
    .expect("Failed to list deployments")["items"]
    .as_array()
    .into_iter()
    .flatten()
    .find(|item| item["metadata"]["name"] == "e2e-manage-web")
    .cloned()
    .expect("e2e-manage-web deployment should exist")
}

#[tokio::test]
async fn manage_workload_round_trip() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig = require_cluster!("manage_workload_round_trip");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-manage".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig.clone()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let application = deploy_manifests(
    &client,
    &cluster.id,
    "e2e-manage-web",
    DEPLOYMENT,
  )
  .await
  .expect("Failed to deploy fixture");

  // Scale 1 -> 2.
  let update = client
    .execute(ScaleClusterWorkload {
      cluster: cluster.id.clone(),
      kind: "deployments".to_string(),
      name: "e2e-manage-web".to_string(),
      replicas: 2,
      namespace: None,
    })
    .await
    .expect("Failed to start scale");
  await_update(&client, &update.id)
    .await
    .expect("Scale did not succeed");
  assert_eq!(
    deployment_json(&client, &cluster.id).await["spec"]["replicas"],
    2,
    "Scale should raise spec.replicas to 2"
  );

  // Rolling restart stamps the pod template with a restartedAt
  // annotation - that is the observable effect.
  let update = client
    .execute(RestartClusterWorkload {
      cluster: cluster.id.clone(),
      kind: "deployments".to_string(),
      name: "e2e-manage-web".to_string(),
      namespace: None,
    })
    .await
    .expect("Failed to start restart");
  await_update(&client, &update.id)
    .await
    .expect("Restart did not succeed");
  let restarted_at = deployment_json(&client, &cluster.id).await
    ["spec"]["template"]["metadata"]["annotations"]
    ["kubectl.kubernetes.io/restartedAt"]
    .clone();
  assert!(
    restarted_at.is_string(),
    "rollout restart should stamp restartedAt, got {restarted_at}"
  );

  // Rollout only accepts workload kinds. The refusal surfaces either
  // as a request error or as a failed Update, depending on whether the
  // execution was queued before resolving.
  match client
    .execute(RestartClusterWorkload {
      cluster: cluster.id.clone(),
      kind: "pods".to_string(),
      name: "anything".to_string(),
      namespace: None,
    })
    .await
  {
    Err(_) => {}
    Ok(update) => {
      let update = finished_update(&client, &update.id)
        .await
        .expect("Refused restart update never finished");
      assert!(
        !update.success,
        "rollout restart on pods must be refused"
      );
    }
  }

  // Edit the object through ApplyClusterObject: change a label.
  let mut edited = deployment_json(&client, &cluster.id).await;
  edited["metadata"]["labels"]["e2e-edited"] =
    serde_json::json!("true");
  // Server bookkeeping the apply path must tolerate being stripped -
  // a stale resourceVersion in particular makes apply fail with a
  // conflict once the controller touches the object.
  if let Some(metadata) = edited["metadata"].as_object_mut() {
    metadata.remove("managedFields");
    metadata.remove("resourceVersion");
  }
  let update = client
    .execute(ApplyClusterObject {
      cluster: cluster.id.clone(),
      contents: edited.to_string(),
      namespace: None,
    })
    .await
    .expect("Failed to start apply");
  await_update(&client, &update.id)
    .await
    .expect("Apply object did not succeed");
  assert_eq!(
    deployment_json(&client, &cluster.id).await["metadata"]["labels"]
      ["e2e-edited"],
    "true",
    "The edited label should be live on the cluster"
  );

  remove_manifests(&client, &application)
    .await
    .expect("Failed to clean up application");
  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}

#[tokio::test]
async fn wait_ready_fails_on_crashlooping_deploy() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig =
    require_cluster!("wait_ready_fails_on_crashlooping_deploy");
  let client = authenticated_client(&env).await.unwrap();

  // A container that exits immediately can still flash Ready before
  // dying (no readiness probe means ready-on-start), which lets
  // `rollout status` win the race. A probe that always fails makes
  // "never becomes ready" deterministic instead.
  let never_ready = DEPLOYMENT
    .replace("e2e-manage-web", "e2e-manage-crash")
    .replace(
      "          command: [\"sh\", \"-c\", \"sleep 3600\"]\n",
      "          command: [\"sh\", \"-c\", \"sleep 3600\"]\n          readinessProbe:\n            exec:\n              command: [\"false\"]\n",
    );

  let cluster = client
    .write(CreateCluster {
      name: "e2e-manage-wait".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  // wait_ready lives on the Application now - the Deploy execution
  // that honors it moved off Cluster entirely.
  let application = client
    .write(CreateApplication {
      name: "e2e-manage-wait".to_string(),
      config: PartialApplicationConfig {
        cluster_id: Some(cluster.id.clone()),
        file_contents: Some(never_ready),
        wait_ready: Some(true),
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
    "wait_ready must fail a deploy whose workload never becomes ready"
  );
  assert!(
    update
      .logs
      .iter()
      .any(|log| log.stage == "Wait For Rollout"),
    "the failure should come from the rollout wait stage"
  );

  remove_manifests(&client, &application.id)
    .await
    .expect("Failed to clean up application");
  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}
