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
use serde::de::DeserializeOwned;
use serde_json::json;

/// Send an auth request, keeping the response body in the error
/// on failure - it carries Core's reason, which is the whole point
/// of a bootstrap smoke test.
async fn post_auth<T: DeserializeOwned>(
  req: reqwest::RequestBuilder,
  what: &str,
) -> anyhow::Result<T> {
  let res = req
    .send()
    .await
    .with_context(|| format!("Failed to reach Core for {what}"))?;
  let status = res.status();
  let body = res
    .text()
    .await
    .with_context(|| format!("Failed to read {what} response"))?;
  if !status.is_success() {
    anyhow::bail!("{what} returned {status}: {body}");
  }
  serde_json::from_str(&body).with_context(|| {
    format!("Failed to parse {what} response: {body}")
  })
}

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
///
/// Core serves auth at `/auth/login` and `/auth/manage`
/// (KomodoClient's `auth_login` posts to `/auth`, which this
/// server layout answers with 405), so both calls are raw.
pub async fn authenticated_client(
  env: &E2eEnv,
) -> anyhow::Result<KomodoClient> {
  let http = reqwest::Client::new();

  let login: JwtOrTwoFactor = post_auth(
    http
      .post(format!("{}/auth/login", env.address))
      .json(&json!({
        "type": "LoginLocalUser",
        "params": LoginLocalUser {
          username: env.username.clone(),
          password: env.password.clone(),
        }
      })),
    "LoginLocalUser",
  )
  .await?;
  let jwt = match login {
    JwtOrTwoFactor::Jwt(res) => res.jwt,
    other => {
      anyhow::bail!(
        "Expected plain jwt login response, got {other:?}"
      )
    }
  };

  // CreateApiKey is authenticated by the jwt in the Authorization header.
  let res: CreateApiKeyResponse = post_auth(
    http
      .post(format!("{}/auth/manage", env.address))
      .header("authorization", format!("Bearer {jwt}"))
      .json(&json!({
        "type": "CreateApiKey",
        "params": { "name": "e2e", "expires": 0 }
      })),
    "CreateApiKey",
  )
  .await?;

  KomodoClient::new(&env.address, res.key, res.secret)
    .with_healthcheck()
    .await
    .context("Client healthcheck failed")
}
