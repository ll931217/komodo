//! End to end test support for Komodo.
//!
//! The tests in `tests/` drive the Komodo client API against a live
//! stack (Core + FerretDB + Periphery + kind cluster) started by
//! `scripts/e2e.sh`. They skip unless `KOMODO_ADDRESS` is set, so a
//! plain `cargo test` outside the harness stays green.

// Used by the integration tests in `tests/`, not by this lib target.
// The `unused_crate_dependencies` lint is per target, so without this
// the lib warns about deps the test binaries genuinely need.
use {bson as _, hex as _, hmac as _, sha2 as _};

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
/// The api key is cached process-wide so the login + CreateApiKey round
/// trip happens once per test binary rather than once per test. The
/// KomodoClient itself is rebuilt each call on purpose: it owns a
/// reqwest connection pool bound to the runtime it was used on, and
/// every #[tokio::test] gets its own runtime, so a shared client fails
/// with "dispatch task is gone" once the first test's runtime is
/// dropped.
///
/// Core serves auth at `/auth/login` and `/auth/manage`
/// (KomodoClient's `auth_login` posts to `/auth`, which this
/// server layout answers with 405), so both calls are raw.
pub async fn authenticated_client(
  env: &E2eEnv,
) -> anyhow::Result<KomodoClient> {
  let credentials = cached_credentials(env).await?;
  KomodoClient::new(
    &env.address,
    &credentials.key,
    &credentials.secret,
  )
  .with_healthcheck()
  .await
  .context("Client healthcheck failed")
}

/// One api key per test binary, minted on first use.
async fn cached_credentials(
  env: &E2eEnv,
) -> anyhow::Result<&'static CreateApiKeyResponse> {
  static CREDENTIALS: tokio::sync::OnceCell<CreateApiKeyResponse> =
    tokio::sync::OnceCell::const_new();
  CREDENTIALS.get_or_try_init(|| create_api_key(env)).await
}

async fn create_api_key(
  env: &E2eEnv,
) -> anyhow::Result<CreateApiKeyResponse> {
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

  Ok(res)
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

/// The kind kubeconfig, or None when no cluster is available.
///
/// The harness clears this when it cannot start a cluster, so tests
/// that need a real Kubernetes api server skip instead of failing.
pub fn kubeconfig() -> Option<String> {
  std::env::var("KOMODO_E2E_KUBECONFIG").ok().filter(|path| {
    !path.is_empty() && std::path::Path::new(path).exists()
  })
}

/// Skip guard for tests needing a live cluster. Prints why, so a
/// skipped test is never mistaken for a passing one.
///
/// Takes the test's name rather than deriving it, which would mean
/// adding a crate just to produce a log label.
#[macro_export]
macro_rules! require_cluster {
  ($test:literal) => {
    match $crate::kubeconfig() {
      Some(path) => path,
      None => {
        eprintln!(
          "SKIP {}: no kind cluster available (KOMODO_E2E_KUBECONFIG unset)",
          $test
        );
        return;
      }
    }
  };
}

/// Whether the Server running Periphery has a terraform binary.
///
/// The harness probes for it and exports this, so the terraform
/// execution tests skip on a host without terraform rather than
/// failing on a missing binary.
pub fn terraform_available() -> bool {
  std::env::var("KOMODO_E2E_TERRAFORM")
    .map(|value| value == "1")
    .unwrap_or(false)
}

/// Skip guard for tests that actually run terraform. Prints why, so a
/// skipped test is never mistaken for a passing one.
#[macro_export]
macro_rules! require_terraform {
  ($test:literal) => {
    if !$crate::terraform_available() {
      eprintln!(
        "SKIP {}: no terraform binary available (KOMODO_E2E_TERRAFORM unset)",
        $test
      );
      return;
    }
  };
}

/// Call an execute request as a jwt-authenticated user.
///
/// Same reasoning as [read_as_jwt]: permission tests need a user who
/// holds no api key.
pub async fn execute_as_jwt<T: DeserializeOwned>(
  env: &E2eEnv,
  jwt: &str,
  request_type: &str,
  params: serde_json::Value,
) -> anyhow::Result<T> {
  let res = reqwest::Client::new()
    .post(format!("{}/execute", env.address))
    .header("authorization", format!("Bearer {jwt}"))
    .json(&json!({ "type": request_type, "params": params }))
    .send()
    .await
    .context("Failed to reach /execute")?;
  let status = res.status();
  let body = res
    .text()
    .await
    .context("Failed to read /execute response")?;
  if !status.is_success() {
    anyhow::bail!("{request_type} returned {status}: {body}");
  }
  serde_json::from_str(&body).with_context(|| {
    format!("Failed to parse {request_type} response: {body}")
  })
}

/// Poll an Update until it leaves `InProgress` and return it.
///
/// Execute requests return as soon as the task is spawned, so a
/// rejected execution shows up as a failed Update rather than an error
/// from the HTTP call.
pub async fn finished_update(
  client: &KomodoClient,
  update_id: &str,
) -> anyhow::Result<komodo_client::entities::update::Update> {
  // Generous ceiling: deleting a pod waits out Kubernetes' default
  // 30s graceful termination, and a wait_ready deploy blocks on
  // `kubectl rollout status --timeout 120s`, so anything tighter
  // fails on timing rather than on behaviour. Polling exits as soon
  // as it is done, so a high ceiling costs nothing when the operation
  // is quick.
  const POLLS: usize = 720;
  const INTERVAL_MS: u64 = 250;
  for _ in 0..POLLS {
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
      return Ok(update);
    }
    tokio::time::sleep(std::time::Duration::from_millis(INTERVAL_MS))
      .await;
  }
  anyhow::bail!(
    "Update {update_id} did not complete in {}s",
    POLLS as u64 * INTERVAL_MS / 1000
  )
}

/// Poll an Update until it leaves `InProgress`, then require success.
///
/// Execute requests return as soon as the task is spawned, so any
/// assertion about their effects has to wait for the Update to finish.
pub async fn await_update(
  client: &KomodoClient,
  update_id: &str,
) -> anyhow::Result<()> {
  let update = finished_update(client, update_id).await?;
  if !update.success {
    anyhow::bail!(
      "Update {} finished unsuccessfully: {:#?}",
      update_id,
      update.logs
    );
  }
  Ok(())
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

/// Run a command in a terminal and return the streamed output.
///
/// `/terminal/execute` streams a plain body rather than returning json,
/// so it does not go through [KomodoClient].
pub async fn execute_terminal(
  env: &E2eEnv,
  key: &str,
  secret: &str,
  body: serde_json::Value,
) -> anyhow::Result<String> {
  let res = reqwest::Client::new()
    .post(format!("{}/terminal/execute", env.address))
    .header("x-api-key", key)
    .header("x-api-secret", secret)
    .json(&body)
    .send()
    .await
    .context("Failed to reach /terminal/execute")?;
  let status = res.status();
  let text = res
    .text()
    .await
    .context("Failed to read terminal output stream")?;
  if !status.is_success() {
    anyhow::bail!("Terminal execute returned {status}: {text}");
  }
  Ok(text)
}

/// The cached api key, for endpoints KomodoClient does not cover.
pub async fn api_credentials(
  env: &E2eEnv,
) -> anyhow::Result<(String, String)> {
  let creds = cached_credentials(env).await?;
  Ok((creds.key.clone(), creds.secret.clone()))
}
