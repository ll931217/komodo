//! Reverting to a historical revision: the plan, not the apply.
//!
//! GetUpdateRevertToml hands back the config snapshot an Update can be
//! reverted TO, having checked it is usable. It applies nothing - the
//! TOML goes through the normal sync path, which already diffs before
//! applying and records its own Update, so a revert stays auditable and
//! is itself revertible.
//!
//! The refusal is the important half. Most updates change no config and
//! carry no snapshot; offering an empty TOML as "the previous state"
//! would, for a managed sync, be read as "this sync manages nothing" -
//! i.e. delete everything it owns.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::api::{
  read::{GetUpdateRevertToml, ListUpdates},
  write::{CreateProcedure, DeleteProcedure},
};
use komodo_client::entities::procedure::PartialProcedureConfig;
use komodo_e2e::{authenticated_client, e2e_env};

/// An update with no config snapshot must be refused, and the reason
/// must be readable by whoever is looking at the button.
#[tokio::test]
async fn an_update_with_no_snapshot_is_refused_with_a_reason() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  // Creating a resource produces an update; a Create carries no
  // "previous" config, so it is exactly the no-snapshot case.
  let procedure = client
    .write(CreateProcedure {
      name: "e2e-revert-plan".to_string(),
      config: PartialProcedureConfig::default(),
    })
    .await
    .expect("Failed to create procedure");

  let updates = client
    .read(ListUpdates {
      query: Some(
        serde_json::from_str(&format!(
          r#"{{"target.type":"Procedure","target.id":"{}"}}"#,
          procedure.id
        ))
        .expect("valid query"),
      ),
      page: 0,
    })
    .await
    .expect("Failed to list updates");

  let update = updates
    .updates
    .first()
    .expect("creating a procedure should record an update");

  let plan = client
    .read(GetUpdateRevertToml {
      update: update.id.clone(),
    })
    .await
    .expect("GetUpdateRevertToml should be dispatchable");

  assert!(
    !plan.revertable,
    "an update with no config snapshot must not be offered as \
     revertable - applying its empty TOML to a managed sync reads as \
     'this sync manages nothing'"
  );
  assert!(
    plan.toml.is_empty(),
    "a refused plan must carry no TOML, or a caller could apply it anyway"
  );
  assert!(
    plan.reason.contains("no config snapshot"),
    "the reason must say what is wrong in words an operator can act \
     on, got: {}",
    plan.reason
  );

  client
    .write(DeleteProcedure {
      id: procedure.id.clone(),
    })
    .await
    .ok();
}

/// A nonexistent update id must error, not answer "not revertable" -
/// those are different problems and conflating them hides a bad id.
#[tokio::test]
async fn an_unknown_update_is_an_error_not_a_refusal() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let result = client
    .read(GetUpdateRevertToml {
      update: "000000000000000000000000".to_string(),
    })
    .await;

  assert!(
    result.is_err(),
    "an unknown update id must error rather than report a clean \
     'nothing to revert to', which would hide a typo'd id"
  );
}
