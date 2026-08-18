//! Build cancellation.
//!
//! Build does NOT use the generic per-execution cancel flow the stack
//! test covers. RunBuild watches `build_cancel_channel` (a tokio
//! broadcast in bin/core/src/helpers/channel.rs) and, on a hit, sends a
//! dedicated `api::build::CancelBuild` to periphery, which looks the
//! build id up in its own `build_cancel_cache`. Two ids, two caches, a
//! broadcast in between - none of it shared with the stack path. So
//! proving the stack cancel works says nothing about this one, which is
//! why it gets its own test.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  api::{
    execute::{CancelBuild, RunBuild},
    read::ListServers,
    write::{CreateBuild, CreateBuilder, DeleteBuild, DeleteBuilder},
  },
  entities::{
    SystemCommand,
    build::PartialBuildConfig,
    builder::{_PartialServerBuilderConfig, PartialBuilderConfig},
  },
};
use komodo_e2e::{authenticated_client, e2e_env, finished_update};

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

/// A cancel must kill the command running on the host, not merely mark
/// the Update cancelled.
///
/// Three deliberate choices here:
///
/// - **`dockerfile` contents, no repo.** Core skips the clone entirely
///   when a build has neither a repo nor files-on-host
///   (bin/core/src/api/execute/build.rs:254), and periphery writes the
///   Dockerfile itself. So the test needs no reachable git remote.
/// - **`pre_build` as the thing to interrupt.** It runs with the same
///   `CancellationToken` as the `docker build` itself
///   (bin/periphery/src/api/build/mod.rs:283 vs :363), and periphery has
///   already registered that token in `build_cancel_cache` by then. It
///   holds the build open for a known duration without depending on how
///   slow a real image build happens to be on the runner.
/// - **A sleep duration as the pgrep marker.** Periphery runs as a host
///   process in the e2e stack, so the test can see its children, and an
///   odd duration cannot collide with another test.
#[tokio::test]
async fn cancel_build_kills_the_command_on_the_host() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id;

  let builder = client
    .write(CreateBuilder {
      name: "e2e-build-cancel-builder".to_string(),
      config: PartialBuilderConfig::Server(
        _PartialServerBuilderConfig {
          server_ids: Some(vec![server_id]),
        },
      ),
    })
    .await
    .expect("Failed to create builder");

  let marker = "sleep 51924";

  let created = client
    .write(CreateBuild {
      name: "e2e-build-cancel".to_string(),
      config: PartialBuildConfig {
        builder_id: Some(builder.id.clone()),
        // No repo and no files_on_host: periphery writes this itself.
        dockerfile: Some("FROM busybox\nRUN true\n".to_string()),
        pre_build: Some(SystemCommand {
          path: String::new(),
          command: marker.to_string(),
          shell_mode: false,
        }),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create build");

  let run = client
    .execute(RunBuild {
      build: created.id.clone(),
    })
    .await
    .expect("Failed to start build");

  // Wait for pre_build to actually be on the host. Cancelling before it
  // starts would pass without proving anything.
  let mut running = false;
  for _ in 0..80 {
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    if pgrep(marker) {
      running = true;
      break;
    }
  }
  assert!(
    running,
    "pre_build never started, so there is nothing to cancel and this \
     test cannot prove anything"
  );

  client
    .execute(CancelBuild {
      build: created.id.clone(),
    })
    .await
    .expect("CancelBuild should be dispatchable while building");

  let finished = finished_update(&client, &run.id)
    .await
    .expect("The build update should finish after a cancel");
  assert!(
    !finished.success,
    "a cancelled build must not report success: {finished:?}"
  );
  assert!(
    finished.was_cancelled(),
    "a cancelled build must be distinguishable from a failed one, but \
     was_cancelled() is false. Logs: {:?}",
    finished.logs.iter().map(|l| &l.stage).collect::<Vec<_>>()
  );

  // The point of the exercise: the process is gone.
  let mut gone = false;
  for _ in 0..40 {
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    if !pgrep(marker) {
      gone = true;
      break;
    }
  }
  assert!(
    gone,
    "the pre_build command survived the cancel - the cancel reached \
     Core but not the host"
  );

  client.write(DeleteBuild { id: created.id }).await.ok();
  client.write(DeleteBuilder { id: builder.id }).await.ok();
}
