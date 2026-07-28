//! Smoke test for the e2e harness itself: authenticate through the
//! full local-login -> api key -> client flow, then assert the
//! init-registered Server is listed.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::api::read::ListServers;
use komodo_e2e::{authenticated_client, e2e_env};

#[tokio::test]
async fn smoke_auth_and_list_first_server() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping e2e smoke test");
    return;
  };

  let client = authenticated_client(&env).await.unwrap();

  let servers = client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers");

  assert!(
    !servers.is_empty(),
    "Expected the init-registered first server to be listed, got none"
  );
}
