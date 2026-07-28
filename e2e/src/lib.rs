//! End to end test support for Komodo.
//!
//! The tests in `tests/` drive the Komodo client API against a live
//! stack (Core + FerretDB + Periphery + kind cluster) started by
//! `scripts/e2e.sh`. They skip unless `KOMODO_ADDRESS` is set, so a
//! plain `cargo test` outside the harness stays green.

use anyhow::Context;
use komodo_client::KomodoClient;
use komodo_client::entities::update::UpdateStatus;
use mogh_auth_client::api::{
  login::{
    JwtOrTwoFactor, JwtResponse, LoginLocalUser, SignUpLocalUser,
  },
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
/// Cached process-wide: every test in a binary shares one client, so
/// the login + CreateApiKey round trip happens once instead of once per
/// test (which also stopped accumulating identically named keys).
///
/// Core serves auth at `/auth/login` and `/auth/manage`
/// (KomodoClient's `auth_login` posts to `/auth`, which this
/// server layout answers with 405), so both calls are raw.
pub async fn authenticated_client(
  env: &E2eEnv,
) -> anyhow::Result<KomodoClient> {
  static CLIENT: tokio::sync::OnceCell<KomodoClient> =
    tokio::sync::OnceCell::const_new();
  CLIENT
    .get_or_try_init(|| create_authenticated_client(env))
    .await
    .cloned()
}

async fn create_authenticated_client(
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

/// Sign up a fresh non-admin local user and return their jwt.
///
/// The harness sets `KOMODO_ENABLE_NEW_USERS` so the account is
/// enabled but holds no permissions on any resource.
pub async fn non_admin_jwt(
  env: &E2eEnv,
  username: &str,
) -> anyhow::Result<String> {
  // Unlike LoginLocalUser, signup responds with a bare JwtResponse
  // rather than the tagged JwtOrTwoFactor.
  let signup: JwtResponse = post_auth(
    reqwest::Client::new()
      .post(format!("{}/auth/login", env.address))
      .json(&json!({
        "type": "SignUpLocalUser",
        "params": SignUpLocalUser {
          username: username.to_string(),
          password: "e2e-nobody-password".to_string(),
        }
      })),
    "SignUpLocalUser",
  )
  .await?;
  Ok(signup.jwt)
}

/// Poll an Update until it leaves `InProgress`, then require success.
///
/// Execute requests return as soon as the task is spawned, so any
/// assertion about their effects has to wait for the Update to finish.
pub async fn await_update(
  client: &KomodoClient,
  update_id: &str,
) -> anyhow::Result<()> {
  for _ in 0..60 {
    let update = client
      .read(komodo_client::api::read::GetUpdate {
        id: update_id.to_string(),
      })
      .await
      .map_err(|e| anyhow::anyhow!("{e:#}"))
      .context("Failed to read update")?;
    if !matches!(
      update.status,
      UpdateStatus::InProgress | UpdateStatus::Queued
    ) {
      if !update.success {
        anyhow::bail!(
          "Update {} finished unsuccessfully: {:#?}",
          update_id,
          update.logs
        );
      }
      return Ok(());
    }
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
  }
  anyhow::bail!("Update {update_id} did not complete in 15s")
}

/// Call a read request as a jwt-authenticated user.
///
/// [KomodoClient] only sends api key headers, and minting an api key
/// for another user requires a service user, so permission tests drive
/// the read endpoint directly with the user's jwt.
pub async fn read_as_jwt<T: DeserializeOwned>(
  env: &E2eEnv,
  jwt: &str,
  request_type: &str,
  params: serde_json::Value,
) -> anyhow::Result<T> {
  let res = reqwest::Client::new()
    .post(format!("{}/read", env.address))
    .header("authorization", format!("Bearer {jwt}"))
    .json(&json!({ "type": request_type, "params": params }))
    .send()
    .await
    .context("Failed to reach /read")?;
  let status = res.status();
  let body =
    res.text().await.context("Failed to read /read response")?;
  if !status.is_success() {
    anyhow::bail!("{request_type} returned {status}: {body}");
  }
  serde_json::from_str(&body).with_context(|| {
    format!("Failed to parse {request_type} response: {body}")
  })
}
