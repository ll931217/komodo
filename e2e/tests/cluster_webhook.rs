//! The deploy webhook: a signed push triggers a Deploy, an unsigned or
//! wrongly-signed one does not.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use hmac::{Hmac, KeyInit as _, Mac};
use komodo_client::{
  KomodoClient,
  api::{
    read::{ListServers, ListUpdates},
    write::{CreateCluster, DeleteCluster},
  },
  entities::cluster::PartialClusterConfig,
};
use komodo_e2e::{authenticated_client, e2e_env, require_cluster};
use sha2::Sha256;

/// Matches KOMODO_WEBHOOK_SECRET in scripts/e2e.sh.
const SECRET: &str = "e2e-webhook-secret";

/// A github push body. The branch has to match the Cluster's.
const PUSH_BODY: &str = r#"{"ref":"refs/heads/main"}"#;

fn signature(secret: &str, body: &str) -> String {
  let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
    .expect("Failed to build hmac");
  mac.update(body.as_bytes());
  format!(
    "sha256={}",
    hex::encode(mac.finalize().into_bytes().as_slice())
  )
}

async fn post_webhook(
  address: &str,
  cluster_id: &str,
  signature: &str,
) -> reqwest::StatusCode {
  reqwest::Client::new()
    .post(format!("{address}/listener/github/cluster/{cluster_id}"))
    .header("x-github-event", "push")
    .header("x-hub-signature-256", signature)
    .header("content-type", "application/json")
    .body(PUSH_BODY.to_string())
    .send()
    .await
    .expect("Failed to reach the webhook listener")
    .status()
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

/// Count DeployCluster updates for this Cluster.
async fn deploy_count(
  client: &KomodoClient,
  cluster_id: &str,
) -> usize {
  client
    .read(ListUpdates {
      query: Some(bson::doc! {
        "operation": "DeployCluster",
        "target.id": cluster_id,
      }),
      page: 0,
    })
    .await
    .expect("Failed to list updates")
    .updates
    .len()
}

#[tokio::test]
async fn signed_webhook_triggers_deploy() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let kubeconfig = require_cluster!("signed_webhook_triggers_deploy");
  let client = authenticated_client(&env).await.unwrap();

  let cluster = client
    .write(CreateCluster {
      name: "e2e-webhook".to_string(),
      config: PartialClusterConfig {
        server_id: Some(server_id(&client).await),
        kubeconfig_path: Some(kubeconfig),
        file_contents: Some(
          "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: e2e-webhook-cm\ndata:\n  hello: world\n"
            .to_string(),
        ),
        branch: Some("main".to_string()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create cluster");

  assert_eq!(
    deploy_count(&client, &cluster.id).await,
    0,
    "No deploy should have run yet"
  );

  // Wrong secret: rejected, and nothing deployed.
  let status = post_webhook(
    &env.address,
    &cluster.id,
    &signature("nope", PUSH_BODY),
  )
  .await;
  assert!(
    !status.is_success(),
    "A wrongly signed webhook must be refused, got {status}"
  );

  // No signature at all: also refused.
  let status = post_webhook(&env.address, &cluster.id, "").await;
  assert!(
    !status.is_success(),
    "An unsigned webhook must be refused, got {status}"
  );

  assert_eq!(
    deploy_count(&client, &cluster.id).await,
    0,
    "Refused webhooks must not have deployed anything"
  );

  // Correctly signed: accepted, and a Deploy update appears.
  let status = post_webhook(
    &env.address,
    &cluster.id,
    &signature(SECRET, PUSH_BODY),
  )
  .await;
  assert!(
    status.is_success(),
    "A correctly signed webhook should be accepted, got {status}"
  );

  // The listener spawns the deploy, so wait for the audit record.
  let mut deploys = 0;
  for _ in 0..60 {
    deploys = deploy_count(&client, &cluster.id).await;
    if deploys > 0 {
      break;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
  }
  assert!(
    deploys > 0,
    "A signed webhook should have produced a DeployCluster update"
  );

  client
    .write(DeleteCluster { id: cluster.id })
    .await
    .expect("Failed to clean up cluster");
}
