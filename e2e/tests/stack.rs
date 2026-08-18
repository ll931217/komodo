//! Stack cancellation. Nothing here deploys a stack - proving a cancel
//! actually kills `docker compose` on the host needs a long-running
//! deploy against a real docker daemon, which is the other half of
//! planning-734.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  api::{
    execute::CancelStack,
    read::ListServers,
    write::{CreateStack, DeleteStack},
  },
  entities::stack::PartialStackConfig,
};
use komodo_e2e::{authenticated_client, e2e_env, finished_update};

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

/// CancelStack is a `#[derive(Resolve)]` dispatch variant plus eight
/// registration sites; miss one and it compiles fine and fails when
/// someone calls it. This is the cheap end-to-end proof that the wiring
/// is reachable from the API at all.
#[tokio::test]
async fn cancel_stack_is_reachable_and_benign_when_idle() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = first_server_id(&client).await;

  let created = client
    .write(CreateStack {
      name: "e2e-stack-cancel".to_string(),
      config: PartialStackConfig {
        server_id: Some(server_id),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create stack");

  let update = client
    .execute(CancelStack {
      stack: created.id.clone(),
    })
    .await
    .expect("CancelStack should be dispatchable");
  let update = finished_update(&client, &update.id)
    .await
    .expect("CancelStack update should finish");

  // Cancelling something that is not deploying is not a failure - a
  // deploy that finished a moment ago looks identical to one that never
  // started, and neither is the caller's mistake.
  assert!(
    update.success,
    "CancelStack on an idle resource should succeed, got: {update:?}"
  );
  let logs = update
    .logs
    .iter()
    .map(|log| log.stdout.clone())
    .collect::<Vec<_>>()
    .join("\n");
  assert!(
    logs.contains("not currently deploying"),
    "expected the idle message, got: {logs}"
  );

  client
    .write(DeleteStack {
      id: created.id.clone(),
    })
    .await
    .ok();
}
