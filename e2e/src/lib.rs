//! End to end test support for Komodo.
//!
//! The tests in `tests/` drive the Komodo client API against a live
//! stack (Core + FerretDB + Periphery + kind cluster) started by
//! `scripts/e2e.sh`. They skip unless `KOMODO_ADDRESS` is set, so a
//! plain `cargo test` outside the harness stays green.

use anyhow::Context;
use komodo_client::KomodoClient;
use mogh_auth_client::api::{
  login::{JwtOrTwoFactor, LoginLocalUser},
  manage::CreateApiKeyResponse,
};
use serde_json::json;

pub struct E2eEnv {
  pub address: String,
  pub username: String,
  pub password: String,
}

/// Returns None (callers skip) when the e2e stack env is not present.
pub fn e2e_env() -> Option<E2eEnv> {
  let address = std::env::var("KOMODO_ADDRESS").ok()?;
  Some(E2eEnv {
    address,
    username: std::env::var("KOMODO_E2E_USERNAME")
      .unwrap_or_else(|_| "e2e-admin".to_string()),
    password: std::env::var("KOMODO_E2E_PASSWORD")
      .unwrap_or_else(|_| "e2e-password".to_string()),
  })
}

/// Login as the init admin local user, mint an api key,
/// and return an authenticated client.
pub async fn authenticated_client(
  env: &E2eEnv,
) -> anyhow::Result<KomodoClient> {
  // Login endpoint takes no api key.
  let anon = KomodoClient::new(&env.address, "", "");
  let jwt = match anon
    .auth_login(LoginLocalUser {
      username: env.username.clone(),
      password: env.password.clone(),
    })
    .await
    .context("Failed to login local user")?
  {
    JwtOrTwoFactor::Jwt(res) => res.jwt,
    other => {
      anyhow::bail!(
        "Expected plain jwt login response, got {other:?}"
      )
    }
  };

  // CreateApiKey is a manage request authenticated by the jwt in the
  // Authorization header, which KomodoClient does not send - one raw call.
  let res: CreateApiKeyResponse = reqwest::Client::new()
    .post(format!("{}/auth", env.address))
    .header("authorization", format!("Bearer {jwt}"))
    .json(&json!({
      "type": "CreateApiKey",
      "params": { "name": "e2e", "expires": 0 }
    }))
    .send()
    .await
    .context("Failed to reach /auth for CreateApiKey")?
    .error_for_status()
    .context("CreateApiKey returned error status")?
    .json()
    .await
    .context("Failed to parse CreateApiKeyResponse")?;

  KomodoClient::new(&env.address, res.key, res.secret)
    .with_healthcheck()
    .await
    .context("Client healthcheck failed")
}
