//! The agent-facing Cluster API: selector / limit / summary listing,
//! describe, events, stateless exec, log windows, preview applies and
//! kind discovery.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    execute::{
      ApplyClusterObject, DiffClusterObject, ExecClusterPod,
    },
    read::{
      DescribeClusterResource, GetClusterEvents, GetClusterPodLog,
      InspectClusterResource, ListClusterApiResources,
      ListClusterResources, ListServers,
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

/// Two labelled two-container pods, so a label selector, a limit and
/// `all_containers` each have something to discriminate, plus a
/// ConfigMap to preview edits against.
const MANIFESTS: &str = r#"apiVersion: v1
kind: Pod
metadata:
  name: e2e-agent-pod-a
  labels:
    app: e2e-agent
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
---
apiVersion: v1
kind: Pod
metadata:
  name: e2e-agent-pod-b
  labels:
    app: e2e-agent
spec:
  restartPolicy: Never
  containers:
    - name: only
      image: busybox:1.36
      imagePullPolicy: IfNotPresent
      command: ["sh", "-c", "sleep 3600"]
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: e2e-agent-config
data:
  answer: original
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

fn pods(cluster: &str) -> ListClusterResources {
  ListClusterResources {
    cluster: cluster.to_string(),
    kind: "pods".to_string(),
    namespace: None,
    all_namespaces: false,
    label_selector: None,
    field_selector: None,
    limit: None,
    summary: false,
  }
}

async fn await_pods_running(client: &KomodoClient, cluster: &str) {
  for _ in 0..240 {
    let listing = client
      .read(pods(cluster))
      .await
      .expect("Failed to list pods");
    let running =
      ["e2e-agent-pod-a", "e2e-agent-pod-b"].iter().all(|name| {
        listing["items"]
          .as_array()
          .into_iter()
          .flatten()
          .find(|item| item["metadata"]["name"] == *name)
          .map(|pod| pod["status"]["phase"] == "Running")
          .unwrap_or(false)
      });
    if running {
      return;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
  }
  panic!("The e2e-agent pods never both reached Running");
}

#[tokio::test]
async fn agent_query_and_preview_surface() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig =
    require_cluster!("agent_query_and_preview_surface");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-agent-api".to_string(),
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
    "e2e-agent-api-objects",
    MANIFESTS,
  )
  .await
  .expect("Failed to deploy the manifests");

  await_pods_running(&client, &cluster.id).await;

  // ==== selectors, limit, summary ====

  let matching = client
    .read(ListClusterResources {
      label_selector: Some("app=e2e-agent".to_string()),
      ..pods(&cluster.id)
    })
    .await
    .expect("Failed to list by label selector");
  assert_eq!(
    matching["items"].as_array().map(Vec::len),
    Some(2),
    "app=e2e-agent should match exactly the two pods: {matching}"
  );

  // A selector that matches nothing must come back empty rather than
  // as the full collection - the failure mode an agent cannot see.
  let none = client
    .read(ListClusterResources {
      label_selector: Some("app=nothing-here".to_string()),
      ..pods(&cluster.id)
    })
    .await
    .expect("Failed to list by label selector");
  assert_eq!(
    none["items"].as_array().map(Vec::len),
    Some(0),
    "A non-matching selector must return no items: {none}"
  );

  let running = client
    .read(ListClusterResources {
      field_selector: Some("status.phase=Running".to_string()),
      label_selector: Some("app=e2e-agent".to_string()),
      ..pods(&cluster.id)
    })
    .await
    .expect("Failed to list by field selector");
  assert_eq!(
    running["items"].as_array().map(Vec::len),
    Some(2),
    "Both pods are Running: {running}"
  );

  let limited = client
    .read(ListClusterResources {
      label_selector: Some("app=e2e-agent".to_string()),
      limit: Some(1),
      ..pods(&cluster.id)
    })
    .await
    .expect("Failed to list with a limit");
  assert_eq!(
    limited["items"].as_array().map(Vec::len),
    Some(1),
    "limit: 1 should cut the listing to one item: {limited}"
  );
  assert_eq!(
    limited["komodo_remaining_items"], 1,
    "The cut-off count must be reported, or the caller reads a truncated list as complete: {limited}"
  );

  let summarized = client
    .read(ListClusterResources {
      label_selector: Some("app=e2e-agent".to_string()),
      summary: true,
      ..pods(&cluster.id)
    })
    .await
    .expect("Failed to list summaries");
  let row = &summarized["items"][0];
  assert!(
    row["name"].is_string() && row["namespace"].is_string(),
    "A summary row carries name and namespace: {row}"
  );
  assert!(
    row["spec"].is_null() && row["metadata"].is_null(),
    "A summary row must not carry the full object: {row}"
  );

  // ==== describe and events ====

  let describe = client
    .read(DescribeClusterResource {
      cluster: cluster.id.clone(),
      kind: "pods".to_string(),
      name: "e2e-agent-pod-a".to_string(),
      namespace: None,
    })
    .await
    .expect("Failed to describe the pod");
  assert!(
    describe.contains("e2e-agent-pod-a")
      && describe.contains("Containers:"),
    "describe should be kubectl's rendered text: {describe}"
  );

  let events = client
    .read(GetClusterEvents {
      cluster: cluster.id.clone(),
      namespace: None,
      all_namespaces: false,
      for_object: Some("e2e-agent-pod-a".to_string()),
      for_kind: Some("Pod".to_string()),
      limit: Some(20),
    })
    .await
    .expect("Failed to read events");
  let rows = events
    .as_array()
    .expect("Events should be an array of rows");
  assert!(
    !rows.is_empty(),
    "A pod that reached Running has events: {events}"
  );
  assert!(
    rows.iter().all(|row| row["object"] == "e2e-agent-pod-a"),
    "for_object must filter to that object only: {events}"
  );
  assert!(
    rows.iter().all(|row| row["reason"].is_string()),
    "Event rows carry a reason: {events}"
  );

  // ==== stateless exec ====

  let update = client
    .execute(ExecClusterPod {
      cluster: cluster.id.clone(),
      pod: "e2e-agent-pod-a".to_string(),
      container: Some("first".to_string()),
      namespace: None,
      command: "echo exec-worked && exit 0".to_string(),
    })
    .await
    .expect("Failed to start exec");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Exec update never finished");
  assert!(update.success, "Exec should succeed: {:#?}", update.logs);
  assert!(
    update
      .logs
      .iter()
      .any(|log| log.stdout.contains("exec-worked")),
    "The command's stdout must come back: {:#?}",
    update.logs
  );

  // A non-zero exit is reported as a failure, not swallowed.
  let update = client
    .execute(ExecClusterPod {
      cluster: cluster.id.clone(),
      pod: "e2e-agent-pod-a".to_string(),
      container: Some("first".to_string()),
      namespace: None,
      command: "exit 7".to_string(),
    })
    .await
    .expect("Failed to start exec");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Exec update never finished");
  assert!(
    !update.success,
    "A command exiting 7 must not report success: {:#?}",
    update.logs
  );

  // ==== log windows ====

  let log = client
    .read(GetClusterPodLog {
      cluster: cluster.id.clone(),
      pod: Some("e2e-agent-pod-a".to_string()),
      label_selector: None,
      container: None,
      all_containers: true,
      namespace: None,
      since: None,
      since_time: None,
      tail: None,
      previous: false,
      timestamps: false,
    })
    .await
    .expect("Failed to read all-containers log");
  assert!(
    log.stdout.contains("hello-from-first")
      && log.stdout.contains("hello-from-second"),
    "all_containers should carry both containers: {}",
    log.stdout
  );

  let log = client
    .read(GetClusterPodLog {
      cluster: cluster.id.clone(),
      pod: None,
      label_selector: Some("app=e2e-agent".to_string()),
      container: None,
      all_containers: true,
      namespace: None,
      since: Some("1h".to_string()),
      since_time: None,
      tail: None,
      previous: false,
      timestamps: false,
    })
    .await
    .expect("Failed to read selector log");
  assert!(
    log.stdout.contains("hello-from-first"),
    "A selector read should reach the labelled pods: {}",
    log.stdout
  );

  // ==== dry run and diff ====

  let edited = r#"apiVersion: v1
kind: ConfigMap
metadata:
  name: e2e-agent-config
data:
  answer: changed
"#;

  let update = client
    .execute(ApplyClusterObject {
      cluster: cluster.id.clone(),
      contents: edited.to_string(),
      namespace: None,
      dry_run: true,
    })
    .await
    .expect("Failed to start dry-run apply");
  await_update(&client, &update.id)
    .await
    .expect("The dry-run apply should be accepted");
  let live = client
    .read(InspectClusterResource {
      cluster: cluster.id.clone(),
      kind: "configmaps".to_string(),
      name: "e2e-agent-config".to_string(),
      namespace: None,
    })
    .await
    .expect("Failed to inspect the ConfigMap");
  assert_eq!(
    live["data"]["answer"], "original",
    "A dry run must not persist anything: {live}"
  );

  let update = client
    .execute(DiffClusterObject {
      cluster: cluster.id.clone(),
      contents: edited.to_string(),
      namespace: None,
    })
    .await
    .expect("Failed to start diff");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Diff update never finished");
  assert!(
    update.success,
    "Differences are a result, not a failure: {:#?}",
    update.logs
  );
  let diff = update
    .logs
    .iter()
    .map(|log| log.stdout.clone())
    .collect::<String>();
  assert!(
    diff.contains("changed"),
    "The diff should show the incoming value: {diff}"
  );

  // Diffing the live object against itself reports no changes, so a
  // caller can tell "already applied" from "would change something".
  let unchanged = r#"apiVersion: v1
kind: ConfigMap
metadata:
  name: e2e-agent-config
data:
  answer: original
"#;
  let update = client
    .execute(DiffClusterObject {
      cluster: cluster.id.clone(),
      contents: unchanged.to_string(),
      namespace: None,
    })
    .await
    .expect("Failed to start diff");
  let update = finished_update(&client, &update.id)
    .await
    .expect("Diff update never finished");
  let diff = update
    .logs
    .iter()
    .map(|log| log.stdout.clone())
    .collect::<String>();
  assert!(
    !diff.contains("changed"),
    "An identical manifest must not report the edited value: {diff}"
  );

  // ==== kind discovery ====

  let discovered = client
    .read(ListClusterApiResources {
      cluster: cluster.id.clone(),
      api_group: None,
      namespaced: None,
      search: None,
    })
    .await
    .expect("Failed to list api resources");
  let find = |name: &str| {
    discovered
      .iter()
      .find(|resource| resource.name == name)
      .cloned()
  };
  let pods_kind = find("pods").expect("Every cluster serves pods");
  assert_eq!(pods_kind.kind, "Pod");
  assert!(pods_kind.namespaced, "Pods are namespaced");
  assert!(
    pods_kind.short_names.contains(&"po".to_string()),
    "Short names should come through: {:?}",
    pods_kind.short_names
  );
  assert!(
    pods_kind.verbs.contains(&"list".to_string()),
    "Verbs should come through: {:?}",
    pods_kind.verbs
  );
  let nodes = find("nodes").expect("Every cluster serves nodes");
  assert!(
    !nodes.namespaced,
    "Nodes are cluster-scoped - the answer the hardcoded list can only guess at"
  );

  let apps = client
    .read(ListClusterApiResources {
      cluster: cluster.id.clone(),
      api_group: Some("apps".to_string()),
      namespaced: None,
      search: None,
    })
    .await
    .expect("Failed to list the apps group");
  assert!(
    apps.iter().any(|resource| resource.name == "deployments"),
    "The apps group has deployments"
  );
  assert!(
    apps
      .iter()
      .all(|resource| resource.api_version.starts_with("apps/")),
    "api_group must exclude every other group"
  );

  let cluster_scoped = client
    .read(ListClusterApiResources {
      cluster: cluster.id.clone(),
      api_group: None,
      namespaced: Some(false),
      search: None,
    })
    .await
    .expect("Failed to list cluster-scoped kinds");
  assert!(
    cluster_scoped.iter().any(|r| r.name == "nodes"),
    "nodes is cluster-scoped"
  );
  assert!(
    !cluster_scoped.iter().any(|r| r.name == "pods"),
    "namespaced: false must exclude pods"
  );

  let searched = client
    .read(ListClusterApiResources {
      cluster: cluster.id.clone(),
      api_group: None,
      namespaced: None,
      search: Some("configmap".to_string()),
    })
    .await
    .expect("Failed to search api resources");
  assert!(
    searched.iter().any(|r| r.name == "configmaps"),
    "search should find configmaps"
  );
  assert!(
    !searched.iter().any(|r| r.name == "pods"),
    "search must exclude non-matching kinds"
  );

  remove_manifests(&client, &application)
    .await
    .expect("Failed to clean up Application");

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}

/// Discovery must not advertise kinds every other endpoint refuses -
/// an agent would read the list as the menu and get errors instead.
#[tokio::test]
async fn api_resource_discovery_respects_the_kind_policy() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig = require_cluster!(
    "api_resource_discovery_respects_the_kind_policy"
  );
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-agent-api-policy".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig.clone()),
        cluster_resources: Some(false),
        exclude_kinds: Some(vec!["Secret".to_string()]),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  let discovered = client
    .read(ListClusterApiResources {
      cluster: cluster.id.clone(),
      api_group: None,
      namespaced: None,
      search: None,
    })
    .await
    .expect("Failed to list api resources");

  assert!(
    discovered.iter().any(|r| r.name == "configmaps"),
    "An allowed kind is still advertised"
  );
  assert!(
    !discovered.iter().any(|r| r.name == "secrets"),
    "An excluded kind must not be advertised"
  );
  assert!(
    !discovered.iter().any(|r| r.name == "nodes"),
    "With cluster_resources off, cluster-scoped kinds must not be advertised"
  );

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}
