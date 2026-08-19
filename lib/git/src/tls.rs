//! TLS client certificates and custom CA bundles for git remotes.
//!
//! git takes these as FILE PATHS (`http.sslCert`, `http.sslKey`,
//! `http.sslCAInfo`), so like the ssh key they have to be written to
//! disk. Same bounding as `ssh.rs`: a per-operation directory at 0700,
//! each file at 0600, removed on drop whether the command succeeded,
//! failed or panicked. Only the paths reach the command line - never the
//! PEM contents, which would be world-readable in `ps`.
//!
//! Scoping is per-invocation rather than per-hostname config. Each git
//! command carries its own `-c http.ssl*` flags and targets exactly one
//! remote, so a CA trusted for one provider cannot leak into a clone of
//! another. That is stronger than a `http.<url>.sslCAInfo` entry in a
//! shared config file, which persists and has to be scoped correctly by
//! hand.

use std::path::{Path, PathBuf};

use anyhow::Context;

/// PEM material for one remote, resolved from the provider account.
///
/// `Debug` is safe to derive: this holds only PATHS, never the PEM
/// contents, so nothing secret can reach a debug print.
#[derive(Debug)]
pub struct TlsSession {
  dir: PathBuf,
  cert_path: Option<PathBuf>,
  key_path: Option<PathBuf>,
  ca_path: Option<PathBuf>,
}

impl TlsSession {
  /// `Ok(None)` when nothing TLS-related is configured, so callers do
  /// not have to distinguish "no session" from "an empty session".
  pub async fn create(
    parent: &Path,
    label: &str,
    client_cert: &str,
    client_key: &str,
    ca_bundle: &str,
  ) -> anyhow::Result<Option<Self>> {
    let cert = non_empty(client_cert);
    let key = non_empty(client_key);
    let ca = non_empty(ca_bundle);
    if cert.is_none() && key.is_none() && ca.is_none() {
      return Ok(None);
    }
    // A cert without its key (or vice versa) cannot authenticate, and
    // git's failure for it is an opaque TLS error. Say which half is
    // missing instead.
    match (&cert, &key) {
      (Some(_), None) => anyhow::bail!(
        "A TLS client certificate is configured for this git provider \
         but its private key is missing"
      ),
      (None, Some(_)) => anyhow::bail!(
        "A TLS client key is configured for this git provider but its \
         certificate is missing"
      ),
      _ => {}
    }

    let dir = parent.join(format!("komodo-tls-{label}"));
    // Never merge into a leftover directory - stale material from an
    // earlier run for a different provider would be presented to this
    // remote.
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir)
      .await
      .context("Failed to create the TLS working directory")?;
    crate::ssh::set_mode(&dir, 0o700).await?;

    let mut session = Self {
      dir: dir.clone(),
      cert_path: None,
      key_path: None,
      ca_path: None,
    };
    if let Some(cert) = cert {
      session.cert_path =
        Some(write(&dir, "client.crt", cert).await?);
    }
    if let Some(key) = key {
      session.key_path = Some(write(&dir, "client.key", key).await?);
    }
    if let Some(ca) = ca {
      session.ca_path = Some(write(&dir, "ca.crt", ca).await?);
    }
    Ok(Some(session))
  }

  /// The `-c` flags for this material, or an empty string.
  ///
  /// Returned as flags on the invocation rather than written into the
  /// repo's config: config persists after the clone and would apply to
  /// every later command in that repo, including ones Komodo did not
  /// issue.
  pub fn config_args(&self) -> String {
    let mut args = Vec::new();
    if let Some(cert) = &self.cert_path {
      args.push(format!("-c http.sslCert={}", cert.display()));
    }
    if let Some(key) = &self.key_path {
      args.push(format!("-c http.sslKey={}", key.display()));
    }
    if let Some(ca) = &self.ca_path {
      args.push(format!("-c http.sslCAInfo={}", ca.display()));
    }
    args.join(" ")
  }
}

impl Drop for TlsSession {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.dir);
  }
}

fn non_empty(value: &str) -> Option<&str> {
  let trimmed = value.trim();
  (!trimmed.is_empty()).then_some(trimmed)
}

async fn write(
  dir: &Path,
  name: &str,
  contents: &str,
) -> anyhow::Result<PathBuf> {
  let path = dir.join(name);
  // PEM parsers are unforgiving about a missing final newline.
  let contents = if contents.ends_with('\n') {
    contents.to_string()
  } else {
    format!("{contents}\n")
  };
  tokio::fs::write(&path, contents)
    .await
    .with_context(|| format!("Failed to write {}", path.display()))?;
  crate::ssh::set_mode(&path, 0o600).await?;
  Ok(path)
}

/// Build a session from a resource's execution args.
pub async fn session_for(
  args: &komodo_client::entities::RepoExecutionArgs,
  label: &str,
) -> anyhow::Result<Option<TlsSession>> {
  let Some(tls) = &args.tls else {
    return Ok(None);
  };
  TlsSession::create(
    &std::env::temp_dir(),
    &crate::ssh::safe_label(label),
    &tls.client_cert,
    &tls.client_key,
    &tls.ca_bundle,
  )
  .await
}

/// The `-c` flags for an optional session.
pub fn config_args(session: Option<&TlsSession>) -> String {
  session.map(TlsSession::config_args).unwrap_or_default()
}

#[cfg(test)]
mod tests {
  use super::*;

  const CERT: &str = "-----BEGIN CERTIFICATE-----\nNotARealCert\n-----END CERTIFICATE-----";
  const KEY: &str = "-----BEGIN PRIVATE KEY-----\nNotARealKey\n-----END PRIVATE KEY-----";
  const CA: &str = "-----BEGIN CERTIFICATE-----\nNotARealCA\n-----END CERTIFICATE-----";

  async fn parent() -> PathBuf {
    let p = std::env::temp_dir().join("komodo-tls-tests");
    tokio::fs::create_dir_all(&p).await.unwrap();
    p
  }

  #[tokio::test]
  async fn nothing_configured_means_no_session() {
    let s = TlsSession::create(&parent().await, "none", "", "", "")
      .await
      .unwrap();
    assert!(s.is_none(), "an empty config produced a session");
  }

  /// The PEM must never reach the command line - only its path.
  #[tokio::test]
  async fn the_pem_contents_never_appear_in_the_args() {
    let s =
      TlsSession::create(&parent().await, "leak", CERT, KEY, CA)
        .await
        .unwrap()
        .expect("session");
    let args = s.config_args();
    for secret in ["NotARealCert", "NotARealKey", "NotARealCA"] {
      assert!(
        !args.contains(secret),
        "{secret} leaked into the args: {args}"
      );
    }
    assert!(args.contains("http.sslCert="));
    assert!(args.contains("http.sslKey="));
    assert!(args.contains("http.sslCAInfo="));
  }

  /// A CA alone is the self-signed-host case and must work without a
  /// client certificate.
  #[tokio::test]
  async fn a_ca_bundle_alone_is_valid() {
    let s = TlsSession::create(&parent().await, "ca", "", "", CA)
      .await
      .unwrap()
      .expect("session");
    let args = s.config_args();
    assert!(args.contains("http.sslCAInfo="));
    assert!(
      !args.contains("http.sslCert="),
      "a cert was configured that nobody asked for: {args}"
    );
  }

  /// Half a client credential cannot authenticate, and git's error for
  /// it is opaque. Fail with the half that is missing named.
  #[tokio::test]
  async fn half_a_client_credential_is_rejected_with_a_useful_message()
   {
    let err =
      TlsSession::create(&parent().await, "half", CERT, "", "")
        .await
        .expect_err("a cert with no key must be rejected");
    assert!(
      err.to_string().contains("private key is missing"),
      "unhelpful error: {err}"
    );

    let err =
      TlsSession::create(&parent().await, "half2", "", KEY, "")
        .await
        .expect_err("a key with no cert must be rejected");
    assert!(
      err.to_string().contains("certificate is missing"),
      "unhelpful error: {err}"
    );
  }

  #[tokio::test]
  async fn the_material_is_not_readable_by_other_users() {
    let s =
      TlsSession::create(&parent().await, "perms", CERT, KEY, CA)
        .await
        .unwrap()
        .expect("session");
    for path in [&s.cert_path, &s.key_path, &s.ca_path]
      .into_iter()
      .flatten()
    {
      assert!(
        !crate::ssh::is_readable_by_others(path).unwrap(),
        "{} is group/other readable",
        path.display()
      );
    }
  }

  #[tokio::test]
  async fn dropping_the_session_removes_the_material() {
    let path = {
      let s =
        TlsSession::create(&parent().await, "cleanup", CERT, KEY, CA)
          .await
          .unwrap()
          .expect("session");
      let dir = s.dir.clone();
      assert!(dir.exists(), "setup failed");
      dir
    };
    assert!(
      !path.exists(),
      "TLS material survived at {}",
      path.display()
    );
  }

  /// PEM parsers reject a file whose last line has no newline, which
  /// surfaces as an unhelpful decode error.
  #[tokio::test]
  async fn material_without_a_trailing_newline_is_fixed_up() {
    let s =
      TlsSession::create(&parent().await, "newline", CERT, KEY, CA)
        .await
        .unwrap()
        .expect("session");
    let written =
      tokio::fs::read_to_string(s.cert_path.as_ref().unwrap())
        .await
        .unwrap();
    assert!(!CERT.ends_with('\n'), "the fixture must lack one");
    assert!(written.ends_with('\n'));
  }

  #[tokio::test]
  async fn a_stale_directory_is_replaced_not_merged() {
    let parent = parent().await;
    let dir = parent.join("komodo-tls-stale");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("leftover"), "old").await.unwrap();
    let _s = TlsSession::create(&parent, "stale", CERT, KEY, CA)
      .await
      .unwrap()
      .expect("session");
    assert!(
      !dir.join("leftover").exists(),
      "material from a previous run survived into this session"
    );
  }

  #[test]
  fn no_session_contributes_no_args() {
    assert_eq!(config_args(None), "");
  }
}
