//! Terraform executions against a real terraform binary.
//!
//! Every fixture here uses only the builtin `terraform_data` resource
//! and declares no providers, so `init` downloads nothing and the whole
//! cycle runs on a host with no registry access at all.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    execute::{ApplyTerraform, DestroyTerraform, PlanTerraform},
    read::{GetTerraform, ListServers},
    write::{
      CreateTerraform, CreateVariable, DeleteTerraform,
      UpdateTerraform,
    },
  },
  entities::terraform::{PartialTerraformConfig, TerraformState},
};
use komodo_e2e::{
  authenticated_client, await_update, e2e_env, finished_update,
  require_terraform,
};

/// A unit with one `terraform_data` resource. `input` is what state
/// records, so changing it is what a later plan reports as drift.
fn unit(input: &str) -> String {
  format!(
    "resource \"terraform_data\" \"e2e\" {{\n  input = \"{input}\"\n}}\n"
  )
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

async fn create(
  client: &KomodoClient,
  name: &str,
  config: PartialTerraformConfig,
) -> String {
  let server_id = server_id(client).await;
  client
    .write(CreateTerraform {
      name: name.to_string(),
      config: PartialTerraformConfig {
        server_id: Some(server_id),
        ..config
      },
    })
    .await
    .unwrap_or_else(|e| panic!("Failed to create {name}: {e:#}"))
    .id
}

async fn state(client: &KomodoClient, id: &str) -> TerraformState {
  client
    .read(GetTerraform {
      terraform: id.to_string(),
    })
    .await
    .expect("Failed to read terraform")
    .info
    .state
}

/// Everything the log carries, for assertions about output.
fn log_text(
  update: &komodo_client::entities::update::Update,
) -> String {
  update
    .logs
    .iter()
    .map(|log| {
      format!("{}\n{}\n{}", log.command, log.stdout, log.stderr)
    })
    .collect::<Vec<_>>()
    .join("\n")
}

/// init -> plan -> apply -> destroy, with the state the resource
/// reports checked at each step.
#[tokio::test]
async fn terraform_full_cycle() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  require_terraform!("terraform_full_cycle");
  let client = authenticated_client(&env).await.unwrap();

  let id = create(
    &client,
    "e2e-tf-cycle",
    PartialTerraformConfig {
      file_contents: Some(unit("hello")),
      ..Default::default()
    },
  )
  .await;

  // First plan: nothing applied yet, so there are changes to make.
  let update = client
    .execute(PlanTerraform {
      terraform: id.clone(),
    })
    .await
    .expect("Failed to start plan");
  await_update(&client, &update.id)
    .await
    .expect("Plan should succeed even when it finds changes");
  assert_eq!(
    state(&client, &id).await,
    TerraformState::Drifted,
    "A plan with pending changes is Drifted, not Failed: \
     -detailed-exitcode returns 2 and that is still success"
  );

  let update = client
    .execute(ApplyTerraform {
      terraform: id.clone(),
    })
    .await
    .expect("Failed to start apply");
  await_update(&client, &update.id)
    .await
    .expect("Apply did not succeed");
  assert_eq!(state(&client, &id).await, TerraformState::Ok);

  // Idempotency: nothing changed, so the next plan finds nothing.
  let update = client
    .execute(PlanTerraform {
      terraform: id.clone(),
    })
    .await
    .expect("Failed to start second plan");
  let finished = finished_update(&client, &update.id)
    .await
    .expect("Second plan did not finish");
  assert!(finished.success);
  assert!(
    log_text(&finished).contains("No changes"),
    "Second plan should report no changes, got:\n{}",
    log_text(&finished)
  );
  assert_eq!(state(&client, &id).await, TerraformState::Ok);

  // Drift: change the configuration, and the plan must notice.
  client
    .write(UpdateTerraform {
      id: id.clone(),
      config: PartialTerraformConfig {
        file_contents: Some(unit("changed")),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to update terraform contents");
  let update = client
    .execute(PlanTerraform {
      terraform: id.clone(),
    })
    .await
    .expect("Failed to start drift plan");
  await_update(&client, &update.id)
    .await
    .expect("Drift plan did not succeed");
  assert_eq!(
    state(&client, &id).await,
    TerraformState::Drifted,
    "A changed configuration must surface as Drifted"
  );

  let update = client
    .execute(DestroyTerraform {
      terraform: id.clone(),
    })
    .await
    .expect("Failed to start destroy");
  let finished = finished_update(&client, &update.id)
    .await
    .expect("Destroy did not finish");
  assert!(finished.success, "Destroy failed: {:#?}", finished.logs);
  assert!(
    log_text(&finished).contains("Destroy complete"),
    "Destroy should report completion, got:\n{}",
    log_text(&finished)
  );

  // Nothing left in state: a plan after destroy has changes to make
  // again (it would re-create), which is the observable proof.
  let update = client
    .execute(PlanTerraform {
      terraform: id.clone(),
    })
    .await
    .expect("Failed to start post-destroy plan");
  await_update(&client, &update.id)
    .await
    .expect("Post-destroy plan did not succeed");
  assert_eq!(state(&client, &id).await, TerraformState::Drifted);

  client
    .write(DeleteTerraform { id })
    .await
    .expect("Failed to clean up terraform");
}

/// A unit that cannot succeed must produce a failed Update, not an
/// HTTP error, and must leave the resource marked Failed.
#[tokio::test]
async fn terraform_failure_is_a_failed_update() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  require_terraform!("terraform_failure_is_a_failed_update");
  let client = authenticated_client(&env).await.unwrap();

  let id = create(
    &client,
    "e2e-tf-failure",
    PartialTerraformConfig {
      // `this_is_not_hcl` is a parse error, so terraform exits nonzero
      // on the very first command.
      file_contents: Some("this_is_not_hcl\n".to_string()),
      ..Default::default()
    },
  )
  .await;

  let update = client
    .execute(PlanTerraform {
      terraform: id.clone(),
    })
    .await
    .expect(
      "Execute should accept the request even when it will fail",
    );
  let finished = finished_update(&client, &update.id)
    .await
    .expect("Plan did not finish");
  assert!(
    !finished.success,
    "A terraform error must fail the Update, got:\n{:#?}",
    finished.logs
  );
  assert_eq!(state(&client, &id).await, TerraformState::Failed);

  client
    .write(DeleteTerraform { id })
    .await
    .expect("Failed to clean up terraform");
}

/// A Komodo Variable marked secret is interpolated into the run, and
/// must not appear anywhere in the Update log.
#[tokio::test]
async fn terraform_secrets_are_scrubbed_from_logs() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  require_terraform!("terraform_secrets_are_scrubbed_from_logs");
  let client = authenticated_client(&env).await.unwrap();

  const SECRET: &str = "e2e-terraform-secret-value";

  // Ignore the error: a previous run of this test may have created it.
  let _ = client
    .write(CreateVariable {
      name: "E2E_TF_SECRET".to_string(),
      value: SECRET.to_string(),
      description: String::new(),
      is_secret: true,
    })
    .await;

  let id = create(
    &client,
    "e2e-tf-secret",
    PartialTerraformConfig {
      // The secret reaches terraform as a variable, and terraform
      // echoes variable values into plan output - which is exactly the
      // path that must be scrubbed.
      file_contents: Some(unit("[[E2E_TF_SECRET]]")),
      environment: Some(
        "TF_VAR_secret=[[E2E_TF_SECRET]]\n".to_string(),
      ),
      ..Default::default()
    },
  )
  .await;

  let update = client
    .execute(PlanTerraform {
      terraform: id.clone(),
    })
    .await
    .expect("Failed to start plan");
  let finished = finished_update(&client, &update.id)
    .await
    .expect("Plan did not finish");

  // The assertion below is only meaningful if terraform actually ran:
  // a plan that never started would trivially contain no secret.
  assert!(
    finished.success,
    "Plan did not run, so the scrubbing assertion proves nothing: {:#?}",
    finished.logs
  );
  let logs = log_text(&finished);
  assert!(
    logs.contains("terraform_data"),
    "Expected real plan output to assert against, got:\n{logs}"
  );
  assert!(
    !logs.contains(SECRET),
    "The secret literal must never reach an Update log:\n{logs}"
  );

  client
    .write(DeleteTerraform { id })
    .await
    .expect("Failed to clean up terraform");
}

/// Two runs on one resource must not overlap: they share a working
/// directory and a state file, so the second is rejected busy.
#[tokio::test]
async fn terraform_concurrent_runs_are_rejected() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  require_terraform!("terraform_concurrent_runs_are_rejected");
  let client = authenticated_client(&env).await.unwrap();

  // A trivial unit applies in under a second, so the second request
  // would simply arrive after the first finished and prove nothing.
  // The provisioner makes the first run last long enough for the two
  // to genuinely overlap, and still pulls in no provider.
  let slow_unit = "resource \"terraform_data\" \"e2e\" {\n       input = \"busy\"\n  provisioner \"local-exec\" {\n         command = \"sleep 10\"\n  }\n}\n";
  let id = create(
    &client,
    "e2e-tf-busy",
    PartialTerraformConfig {
      file_contents: Some(slow_unit.to_string()),
      ..Default::default()
    },
  )
  .await;

  let first = client
    .execute(ApplyTerraform {
      terraform: id.clone(),
    })
    .await
    .expect("Failed to start first apply");

  // Fired while the first is still running: one of the two must fail
  // rather than both proceeding against one state file. The sleep is
  // for the guard to be taken, not for the run to finish - the unit
  // stays busy for ten seconds.
  tokio::time::sleep(std::time::Duration::from_secs(2)).await;
  let second = client
    .execute(ApplyTerraform {
      terraform: id.clone(),
    })
    .await
    .expect("Second execute should be accepted, then fail as busy");

  let first = finished_update(&client, &first.id)
    .await
    .expect("First apply did not finish");
  let second = finished_update(&client, &second.id)
    .await
    .expect("Second apply did not finish");

  assert!(
    !(first.success && second.success),
    "Two concurrent applies both succeeded - the action state busy \
     check is not gating them"
  );

  // Clean up whatever landed. The destroy has to FINISH before the
  // delete: the resource is busy while it runs, and deleting a busy
  // resource is refused - which failed this test on its cleanup rather
  // than on anything it was asserting.
  if let Ok(update) = client
    .execute(DestroyTerraform {
      terraform: id.clone(),
    })
    .await
  {
    let _ = finished_update(&client, &update.id).await;
  }
  client
    .write(DeleteTerraform { id })
    .await
    .expect("Failed to clean up terraform");
}
