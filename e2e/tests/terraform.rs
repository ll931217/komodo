//! Terraform resource: CRUD, toml sync round-trip, and permission
//! checks. Nothing here runs terraform - see `terraform_run.rs`.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  api::{
    execute::{CancelTerraform, RunSync},
    read::{
      ExportResourcesToToml, GetTerraform, ListServers,
      ListTerraforms,
    },
    write::{
      CreateResourceSync, CreateTerraform, DeleteResourceSync,
      DeleteTerraform, RenameTerraform, UpdateTerraform,
    },
  },
  entities::{ResourceTarget, terraform::PartialTerraformConfig},
};
use komodo_e2e::{
  authenticated_client, await_update, e2e_env, execute_as_jwt,
  finished_update, non_admin_jwt, read_as_jwt,
};

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

#[tokio::test]
async fn terraform_crud_round_trip() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = first_server_id(&client).await;

  let created = client
    .write(CreateTerraform {
      name: "e2e-tf-crud".to_string(),
      config: PartialTerraformConfig {
        server_id: Some(server_id.clone()),
        run_directory: Some("unit".to_string()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create terraform");

  assert_eq!(created.name, "e2e-tf-crud");
  assert_eq!(created.config.server_id, server_id);
  assert!(
    created.config.managed_state,
    "managed_state must default on: state belongs outside the checkout"
  );

  let listed = client
    .read(ListTerraforms::default())
    .await
    .expect("Failed to list terraforms");
  let item = listed
    .iter()
    .find(|t| t.id == created.id)
    .expect("Created terraform missing from ListTerraforms");
  assert_eq!(item.info.server_id, server_id);
  assert_eq!(item.info.run_directory, "unit");

  // Update merges only set fields.
  let updated = client
    .write(UpdateTerraform {
      id: created.id.clone(),
      config: PartialTerraformConfig {
        run_directory: Some("other-unit".to_string()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to update terraform");
  assert_eq!(updated.config.run_directory, "other-unit");
  assert_eq!(
    updated.config.server_id, server_id,
    "Update must not clear fields absent from the partial config"
  );

  client
    .write(RenameTerraform {
      id: created.id.clone(),
      name: "e2e-tf-crud-renamed".to_string(),
    })
    .await
    .expect("Failed to rename terraform");

  let fetched = client
    .read(GetTerraform {
      terraform: created.id.clone(),
    })
    .await
    .expect("Failed to get terraform after rename");
  assert_eq!(fetched.name, "e2e-tf-crud-renamed");

  client
    .write(DeleteTerraform {
      id: created.id.clone(),
    })
    .await
    .expect("Failed to delete terraform");

  assert!(
    client
      .read(GetTerraform {
        terraform: created.id.clone()
      })
      .await
      .is_err(),
    "Deleted terraform should no longer be readable"
  );
}

/// Export a Terraform to toml, then sync it back from a fresh sync
/// resource. This is the test that catches a missing Terraform entry in
/// any of the manual toml-sync enumeration sites: those compile fine
/// while silently computing deltas that are never applied.
#[tokio::test]
async fn terraform_toml_sync_round_trip() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = first_server_id(&client).await;

  let created = client
    .write(CreateTerraform {
      name: "e2e-tf-toml".to_string(),
      config: PartialTerraformConfig {
        server_id: Some(server_id.clone()),
        run_directory: Some("original".to_string()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create terraform");

  let exported = client
    .read(ExportResourcesToToml {
      targets: vec![ResourceTarget::Terraform(created.id.clone())],
      user_groups: Vec::new(),
      include_variables: false,
      existing: None,
    })
    .await
    .expect("Failed to export terraform toml");

  assert!(
    exported.toml.contains("[[terraform]]"),
    "Export must use the terraform toml header, got:\n{}",
    exported.toml
  );
  assert!(
    exported.toml.contains("run_directory = \"original\""),
    "Export must carry config fields, got:\n{}",
    exported.toml
  );

  // Delete it, then let a sync recreate it from the exported toml.
  client
    .write(DeleteTerraform {
      id: created.id.clone(),
    })
    .await
    .expect("Failed to delete terraform");

  let sync = client
    .write(CreateResourceSync {
      name: "e2e-tf-toml-sync".to_string(),
      config:
        komodo_client::entities::sync::PartialResourceSyncConfig {
          file_contents: Some(exported.toml.clone()),
          ..Default::default()
        },
    })
    .await
    .expect("Failed to create resource sync");

  let update = client
    .execute(RunSync {
      sync: sync.id.clone(),
      resource_type: None,
      resources: None,
      dry_run: false,
      confirm_deletes: false,
    })
    .await
    .expect("Failed to run sync");
  await_update(&client, &update.id)
    .await
    .expect("Sync update did not succeed");

  let synced = client
    .read(GetTerraform {
      terraform: "e2e-tf-toml".to_string(),
    })
    .await
    .expect(
      "Sync did not recreate the Terraform - check that Terraform is \
       registered in every manual toml-sync enumeration site",
    );
  assert_eq!(synced.config.run_directory, "original");
  assert_eq!(
    synced.config.server_id, server_id,
    "Sync must resolve the Server name back to its id"
  );

  client
    .write(DeleteTerraform { id: synced.id })
    .await
    .expect("Failed to clean up synced terraform");
  // The sync resource too, so a rerun against the same stack is not a
  // name conflict.
  client
    .write(DeleteResourceSync { id: sync.id })
    .await
    .expect("Failed to clean up resource sync");
}

#[tokio::test]
async fn terraform_hidden_from_unpermitted_user() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = first_server_id(&client).await;

  let created = client
    .write(CreateTerraform {
      name: "e2e-tf-perms".to_string(),
      config: PartialTerraformConfig {
        server_id: Some(server_id),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create terraform");

  let jwt = non_admin_jwt(&env, "e2e-nobody-terraform")
    .await
    .expect("Failed to create non-admin user");

  // No permission granted: the resource must be invisible...
  let listed: Vec<serde_json::Value> =
    read_as_jwt(&env, &jwt, "ListTerraforms", serde_json::json!({}))
      .await
      .expect("ListTerraforms should succeed but return nothing");
  assert!(
    listed.is_empty(),
    "Non-admin without permission must not see any Terraform, got {listed:?}"
  );

  // ...and fetching it directly must fail.
  let direct = read_as_jwt::<serde_json::Value>(
    &env,
    &jwt,
    "GetTerraform",
    serde_json::json!({ "terraform": created.id }),
  )
  .await;
  assert!(
    direct.is_err(),
    "Non-admin without permission must not read a Terraform directly"
  );

  // Execute is gated too. /execute answers as soon as the task is
  // spawned, so a rejected execution surfaces as a failed Update
  // rather than an HTTP error - either is a refusal, neither is a run.
  let executed = execute_as_jwt::<serde_json::Value>(
    &env,
    &jwt,
    "PlanTerraform",
    serde_json::json!({ "terraform": created.id }),
  )
  .await;
  if let Ok(update) = executed {
    let update_id = update
      .get("_id")
      .and_then(|id| id.get("$oid"))
      .and_then(|id| id.as_str())
      .expect("Execute response should carry an Update id")
      .to_string();
    let finished = finished_update(&client, &update_id)
      .await
      .expect("Update did not finish");
    assert!(
      !finished.success,
      "Non-admin without permission must not execute a Terraform,        got a successful Update: {:#?}",
      finished.logs
    );
  }

  client
    .write(DeleteTerraform { id: created.id })
    .await
    .expect("Failed to clean up terraform");
}

/// CancelTerraform reaches the server and answers, even with nothing
/// running.
///
/// The point is not the message - it is that the request is wired all
/// the way through. A new execute request needs entries in the
/// Execution enum, the ExecuteRequest enum, the procedure match, the
/// permission map and the update-operation map, and the ones that are
/// not exhaustive matches fail at runtime rather than at build time.
/// A round trip that comes back Complete proves every one of them.
///
/// Deliberately does not require the terraform binary: this is about
/// reachability, and terraform_run.rs covers actually running it.
#[tokio::test]
async fn cancel_terraform_is_reachable_and_benign_when_idle() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = first_server_id(&client).await;

  let created = client
    .write(CreateTerraform {
      name: "e2e-tf-cancel".to_string(),
      config: PartialTerraformConfig {
        server_id: Some(server_id),
        run_directory: Some("unit".to_string()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create terraform");

  let update = client
    .execute(CancelTerraform {
      terraform: created.id.clone(),
    })
    .await
    .expect("CancelTerraform should be dispatchable");
  let update = finished_update(&client, &update.id)
    .await
    .expect("CancelTerraform update should finish");

  // Cancelling something that is not running is not a failure - a run
  // that finished a moment ago looks identical to one that never
  // started, and neither is the caller's mistake.
  assert!(
    update.success,
    "CancelTerraform on an idle resource should succeed, got: {update:?}"
  );
  let logs = update
    .logs
    .iter()
    .map(|log| log.stdout.clone())
    .collect::<Vec<_>>()
    .join("\n");
  assert!(
    logs.contains("not currently running"),
    "expected the idle message, got: {logs}"
  );

  client
    .write(DeleteTerraform {
      id: created.id.clone(),
    })
    .await
    .ok();
}
