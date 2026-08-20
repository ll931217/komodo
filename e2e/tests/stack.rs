//! Stack cancellation, both halves: that the verb is reachable and
//! benign when nothing is running, and that a cancel mid-deploy actually
//! kills the command on the host rather than just marking the Update.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  api::{
    execute::{CancelStack, DeployStack},
    read::ListServers,
    write::{CreateStack, DeleteStack},
  },
  entities::{SystemCommand, stack::PartialStackConfig},
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
      cascade: false,
    })
    .await
    .ok();
}

/// The in-flight half: a cancel must actually kill the command running
/// on the host, not merely mark the Update cancelled.
///
/// pre_deploy runs before `docker compose up` and blocks, which is the
/// cheapest way to hold a deploy open long enough to cancel it. The
/// sleep duration doubles as a unique pgrep marker - periphery runs as
/// a host process in the e2e stack, so the test can see its children.
#[tokio::test]
async fn cancel_stack_kills_the_command_on_the_host() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = first_server_id(&client).await;

  // Unique so pgrep cannot match another test's process.
  let marker = "sleep 51923";

  let created = client
    .write(CreateStack {
      name: "e2e-stack-cancel-inflight".to_string(),
      config: PartialStackConfig {
        server_id: Some(server_id),
        file_contents: Some(
          "services:\n  noop:\n    image: busybox\n    command: true\n"
            .to_string(),
        ),
        pre_deploy: Some(SystemCommand {
          path: String::new(),
          command: marker.to_string(),
          shell_mode: false,
        }),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create stack");

  let deploy = client
    .execute(DeployStack {
      stack: created.id.clone(),
      services: Vec::new(),
      stop_time: None,
    })
    .await
    .expect("Failed to start deploy");

  // Wait for the pre_deploy command to actually be running - cancelling
  // before it is on the host would prove nothing.
  let mut running = false;
  for _ in 0..40 {
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    if pgrep(marker) {
      running = true;
      break;
    }
  }
  assert!(
    running,
    "pre_deploy never started; nothing to cancel, so this test cannot \
     prove anything"
  );

  client
    .execute(CancelStack {
      stack: created.id.clone(),
    })
    .await
    .expect("CancelStack should be dispatchable");

  let finished = finished_update(&client, &deploy.id)
    .await
    .expect("The deploy update should finish after a cancel");
  assert!(
    !finished.success,
    "a cancelled deploy must not report success: {finished:?}"
  );
  // Not success is not enough - a cancelled run has to be tellable from
  // a failed one, which is the whole point of CANCELLED_LOG_STAGE. This
  // is the end-to-end check on that; the guard in
  // bin/core/src/api/mod.rs only proves the constant is referenced.
  assert!(
    finished.was_cancelled(),
    "a cancelled deploy must be distinguishable from a failed one, but \
     was_cancelled() is false. Logs: {:?}",
    finished.logs.iter().map(|l| &l.stage).collect::<Vec<_>>()
  );

  // The point of the whole exercise: the process is gone.
  let mut gone = false;
  for _ in 0..20 {
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    if !pgrep(marker) {
      gone = true;
      break;
    }
  }
  assert!(
    gone,
    "the pre_deploy command survived the cancel - the token reached \
     Core but not the host"
  );

  client
    .write(DeleteStack {
      id: created.id,
      cascade: false,
    })
    .await
    .ok();
}

/// True while a process matching `pattern` exists. pgrep excludes
/// itself, so this does not match its own command line.
fn pgrep(pattern: &str) -> bool {
  std::process::Command::new("pgrep")
    .args(["-f", pattern])
    .output()
    .map(|out| {
      !String::from_utf8_lossy(&out.stdout).trim().is_empty()
    })
    .unwrap_or(false)
}
