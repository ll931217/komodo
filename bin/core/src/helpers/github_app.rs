//! GitHub App installation tokens.
//!
//! A GitHub App is the alternative to handing Komodo a long-lived
//! personal access token: an org installs the App, and Komodo mints a
//! token that expires in an hour. The App's private key never leaves
//! Core, and the credential that reaches git is short-lived by
//! construction rather than by policy.
//!
//! Two steps, both required by GitHub:
//!   1. Sign a JWT with the App's RSA key. GitHub caps its lifetime at
//!      10 minutes and rejects a longer one outright.
//!   2. Exchange that JWT for an installation token, which is what
//!      actually authenticates a git operation.
//!
//! The exchange is a network call, so the result is cached until shortly
//! before expiry. Without that, every clone in a sync would mint a fresh
//! token and GitHub would rate-limit the App.

use std::{
  collections::HashMap,
  sync::OnceLock,
  time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// GitHub rejects a JWT whose lifetime exceeds 10 minutes. 9 leaves room
/// for clock skew between Core and GitHub without going over.
const JWT_LIFETIME: Duration = Duration::from_secs(9 * 60);

/// GitHub back-dates `iat` tolerance narrowly, and a Core clock running
/// slightly fast produces a token "issued in the future", which is
/// rejected with an unhelpful 401. Back-dating absorbs that.
const JWT_BACKDATE: Duration = Duration::from_secs(60);

/// Re-mint this far before the installation token actually expires.
/// A token that expires mid-clone fails the clone, and the clone is
/// where it is least convenient to discover that.
const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
  /// Issued at, back-dated.
  iat: u64,
  /// Expiry.
  exp: u64,
  /// The App id.
  iss: String,
}

/// A minted installation token and when it stops being usable.
#[derive(Clone)]
struct CachedToken {
  token: String,
  /// Unix seconds.
  expires_at: u64,
}

type Cache = Mutex<HashMap<String, CachedToken>>;

fn cache() -> &'static Cache {
  static CACHE: OnceLock<Cache> = OnceLock::new();
  CACHE.get_or_init(Default::default)
}

fn now_secs() -> anyhow::Result<u64> {
  Ok(
    SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .context("system clock is before the unix epoch")?
      .as_secs(),
  )
}

/// Sign the App JWT.
///
/// Separate from the exchange so it can be tested without a network:
/// the claims are where the failures live (a too-long lifetime, a clock
/// skew rejection), and both surface from GitHub as a bare 401.
pub fn app_jwt(
  app_id: &str,
  private_key: &str,
) -> anyhow::Result<String> {
  let now = now_secs()?;
  let claims = Claims {
    iat: now.saturating_sub(JWT_BACKDATE.as_secs()),
    exp: now + JWT_LIFETIME.as_secs(),
    iss: app_id.to_string(),
  };
  let key = jsonwebtoken::EncodingKey::from_rsa_pem(
    private_key.as_bytes(),
  )
  .context(
    "The GitHub App private key is not a valid RSA PEM. GitHub issues \
     it in PKCS#1 (BEGIN RSA PRIVATE KEY); a key converted to another \
     format will not load.",
  )?;
  jsonwebtoken::encode(
    &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
    &claims,
    &key,
  )
  .context("Failed to sign the GitHub App JWT")
}

#[derive(Deserialize)]
struct InstallationTokenResponse {
  token: String,
  /// RFC 3339, eg 2026-08-19T01:23:45Z
  expires_at: String,
}

/// Mint (or reuse) an installation token for this App installation.
///
/// `domain` supports GitHub Enterprise: the API base differs, and
/// hard-coding api.github.com would silently send an Enterprise
/// customer's App credentials to github.com.
pub async fn installation_token(
  domain: &str,
  app_id: &str,
  installation_id: &str,
  private_key: &str,
) -> anyhow::Result<String> {
  let key = format!("{domain}/{app_id}/{installation_id}");
  let now = now_secs()?;

  if let Some(cached) = cache().lock().await.get(&key)
    && cached.expires_at > now + REFRESH_MARGIN.as_secs()
  {
    return Ok(cached.token.clone());
  }

  let api_base = if domain == "github.com" {
    "https://api.github.com".to_string()
  } else {
    // GitHub Enterprise Server mounts the API under /api/v3.
    format!("https://{domain}/api/v3")
  };
  let url = format!(
    "{api_base}/app/installations/{installation_id}/access_tokens"
  );

  let jwt = app_jwt(app_id, private_key)?;
  let response = reqwest::Client::new()
    .post(&url)
    .header("Authorization", format!("Bearer {jwt}"))
    .header("Accept", "application/vnd.github+json")
    // GitHub rejects a request with no User-Agent.
    .header("User-Agent", "komodo")
    .send()
    .await
    .context(
      "Failed to reach GitHub to mint an installation token",
    )?;

  let status = response.status();
  let body = response.text().await.unwrap_or_default();
  if !status.is_success() {
    // The JWT is a bearer credential; never echo the request. The body
    // is GitHub's own error text and is safe.
    anyhow::bail!(
      "GitHub refused to mint an installation token for app {app_id} \
       installation {installation_id}: {status} {body}"
    );
  }

  let parsed: InstallationTokenResponse = serde_json::from_str(&body)
    .context(
      "GitHub returned an unexpected installation token body",
    )?;
  let expires_at = parse_expiry(&parsed.expires_at).unwrap_or(
    // A body we cannot parse the expiry from still yields a usable
    // token; treat it as short-lived rather than discarding it.
    now + REFRESH_MARGIN.as_secs() + 60,
  );

  cache().lock().await.insert(
    key,
    CachedToken {
      token: parsed.token.clone(),
      expires_at,
    },
  );
  Ok(parsed.token)
}

/// Parse GitHub's RFC 3339 expiry into unix seconds.
///
/// Hand-rolled rather than pulling in a date crate for one field, and
/// deliberately returns None on anything unexpected so the caller can
/// fall back to a short lifetime instead of failing a working clone.
fn parse_expiry(value: &str) -> Option<u64> {
  // 2026-08-19T01:23:45Z
  let value = value.strip_suffix('Z')?;
  let (date, time) = value.split_once('T')?;
  let mut date = date.split('-');
  let year: i64 = date.next()?.parse().ok()?;
  let month: i64 = date.next()?.parse().ok()?;
  let day: i64 = date.next()?.parse().ok()?;
  let mut time = time.split(':');
  let hour: i64 = time.next()?.parse().ok()?;
  let minute: i64 = time.next()?.parse().ok()?;
  let second: i64 = time.next()?.parse().ok()?;

  // Days from civil, per Howard Hinnant's algorithm - exact for the
  // proleptic Gregorian calendar and free of leap-year edge cases.
  let y = if month <= 2 { year - 1 } else { year };
  let era = if y >= 0 { y } else { y - 399 } / 400;
  let yoe = y - era * 400;
  let mp = (month + 9) % 12;
  let doy = (153 * mp + 2) / 5 + day - 1;
  let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
  let days = era * 146_097 + doe - 719_468;

  u64::try_from(days * 86_400 + hour * 3_600 + minute * 60 + second)
    .ok()
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Expected values computed independently rather than by hand - the
  /// first attempt at the third case was a day out, and a date parser
  /// checked against a hand-arithmetic fixture is checking the fixture.
  #[test]
  fn the_expiry_parser_matches_known_timestamps() {
    // Unix epoch itself.
    assert_eq!(parse_expiry("1970-01-01T00:00:00Z"), Some(0));
    // A leap day, where an off-by-one in the month handling shows up.
    assert_eq!(
      parse_expiry("2024-02-29T12:00:00Z"),
      Some(1_709_208_000)
    );
    assert_eq!(
      parse_expiry("2026-08-19T01:23:45Z"),
      Some(1_787_102_625)
    );
    // The day either side, so a whole-day offset cannot pass.
    assert_eq!(
      parse_expiry("2026-08-20T01:23:45Z"),
      Some(1_787_102_625 + 86_400)
    );
    assert_eq!(
      parse_expiry("2026-08-18T01:23:45Z"),
      Some(1_787_102_625 - 86_400)
    );
    // A year boundary and the leap day's neighbour.
    assert_eq!(
      parse_expiry("2025-01-01T00:00:00Z"),
      Some(1_735_689_600)
    );
    assert_eq!(
      parse_expiry("2024-03-01T12:00:00Z"),
      Some(1_709_208_000 + 86_400)
    );
  }

  /// A body Komodo cannot parse must not fail a clone that has a
  /// perfectly usable token in it.
  #[test]
  fn an_unparseable_expiry_is_none_rather_than_an_error() {
    assert_eq!(parse_expiry("not a date"), None);
    assert_eq!(parse_expiry("2026-08-19 01:23:45"), None, "no T");
    assert_eq!(
      parse_expiry("2026-08-19T01:23:45+00:00"),
      None,
      "no Z"
    );
    assert_eq!(parse_expiry(""), None);
  }

  /// GitHub rejects a JWT whose lifetime exceeds 10 minutes outright,
  /// and the rejection is a bare 401 that says nothing about lifetimes.
  #[test]
  fn the_jwt_lifetime_stays_inside_githubs_limit() {
    let span = JWT_LIFETIME.as_secs() + JWT_BACKDATE.as_secs();
    assert!(
      span <= 600,
      "iat is back-dated by {}s and exp is {}s out, spanning {span}s - \
       GitHub caps the total at 600s",
      JWT_BACKDATE.as_secs(),
      JWT_LIFETIME.as_secs()
    );
  }

  /// Re-minting must start before expiry, or a token can die mid-clone.
  #[test]
  fn the_refresh_margin_leaves_room_before_expiry() {
    assert!(
      REFRESH_MARGIN.as_secs() >= 60,
      "too small a margin re-mints only once the token is nearly dead"
    );
    // GitHub installation tokens last an hour; a margin near that would
    // mean re-minting on every single call.
    assert!(
      REFRESH_MARGIN.as_secs() < 30 * 60,
      "too large a margin re-mints on every call and gets the App \
       rate-limited"
    );
  }

  /// A malformed key must say what shape is expected. GitHub hands out
  /// PKCS#1, and the error from the jwt crate alone does not say so.
  #[test]
  fn a_bad_private_key_explains_the_expected_format() {
    let err = app_jwt("123", "not a pem").expect_err("must fail");
    let text = format!("{err:#}");
    assert!(
      text.contains("PKCS#1"),
      "the error should name the expected key format, got: {text}"
    );
  }
}
