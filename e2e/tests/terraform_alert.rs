//! Drift alerts: raised by a plan that finds pending changes,
//! resolved by a later run that finds none.
//!
//! Unlike every other resource's alerts, these come from a run rather
//! than the monitor's polling loop, so this test drives executions
//! rather than waiting for a probe.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  KomodoClient,
  api::{
    execute::{ApplyTerraform, DestroyTerraform, PlanTerraform},
    read::{ListAlerts, ListServers},
    write::{CreateTerraform, DeleteTerraform},
  },
  entities::{
    alert::{Alert, SeverityLevel},
    terraform::PartialTerraformConfig,
  },
};
use komodo_e2e::{
  authenticated_client, await_update, e2e_env, require_terraform,
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

/// The alert for this resource, if one is open (or resolved).
async fn alert(
  client: &KomodoClient,
  terraform_id: &str,
  resolved: bool,
) -> Option<Alert> {
  client
    .read(ListAlerts {
      query: Some(bson::doc! {
        "data.type": "TerraformUnhealthy",
        "data.data.id": terraform_id,
        "resolved": resolved,
      }),
      page: 0,
    })
    .await
    .expect("Failed to list alerts")
    .alerts
    .into_iter()
    .next()
}

#[tokio::test]
async fn terraform_drift_opens_and_resolves_an_alert() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  require_terraform!("terraform_drift_opens_and_resolves_an_alert");
  let client = authenticated_client(&env).await.unwrap();

  let created = client
    .write(CreateTerraform {
      name: "e2e-tf-alert".to_string(),
      config: PartialTerraformConfig {
        server_id: Some(server_id(&client).await),
        file_contents: Some(
          "resource \"terraform_data\" \"e2e\" {\n  input = \"alert\"\n}\n"
            .to_string(),
        ),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create terraform");

  // Nothing applied yet, so this plan has changes to report.
  let update = client
    .execute(PlanTerraform {
      terraform: created.id.clone(),
    })
    .await
    .expect("Failed to start plan");
  await_update(&client, &update.id)
    .await
    .expect("Plan did not succeed");

  let open = alert(&client, &created.id, false)
    .await
    .expect("A plan with pending changes must open an alert");
  assert_eq!(
    open.level,
    SeverityLevel::Warning,
    "Drift is a warning, not a critical: the run itself succeeded"
  );

  // Applying makes reality match, so the next run resolves it.
  let update = client
    .execute(ApplyTerraform {
      terraform: created.id.clone(),
    })
    .await
    .expect("Failed to start apply");
  await_update(&client, &update.id)
    .await
    .expect("Apply did not succeed");

  assert!(
    alert(&client, &created.id, false).await.is_none(),
    "The alert must not still be open after a successful apply"
  );
  let resolved = alert(&client, &created.id, true)
    .await
    .expect("The alert must be resolved, not deleted");
  assert!(resolved.resolved_ts.is_some());
  // The stored level stays whatever opened the alert - only the
  // notification sent to alerters is downgraded to Ok. Same as the
  // Cluster resolve path; asserting Ok here would be asserting a
  // behaviour Komodo does not have.

  let _ = client
    .execute(DestroyTerraform {
      terraform: created.id.clone(),
    })
    .await;
  client
    .write(DeleteTerraform { id: created.id })
    .await
    .expect("Failed to clean up terraform");
}
