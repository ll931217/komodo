use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::I64;

use super::KomodoWriteRequest;

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/ReencryptSecrets",
  description = "**Admin only.** Re-encrypt every stored secret with the newest configured key.",
  request_body(content = ReencryptSecrets),
  responses(
    (status = 200, description = "What was re-encrypted", body = ReencryptSecretsResponse),
  ),
)]
pub fn reencrypt_secrets() {}

/// **Admin only.** Re-encrypt every stored secret with the newest
/// configured `secret_keys` entry. Response:
/// [ReencryptSecretsResponse].
///
/// Covers secret Variable values and git provider / image registry
/// account tokens, and answers the two questions the encryption
/// design otherwise leaves open:
///
/// - **Rotation.** A value records the key version that wrote it, so
///   adding a key does not re-encrypt anything and every old key has
///   to stay in the config forever - the opposite of what rotating is
///   for. Run this after adding a key and the old one can be dropped.
/// - **Adoption.** Turning encryption on does not reach backwards: a
///   token written before the key existed stays plaintext until
///   something happens to rewrite it. This rewrites it now.
///
/// Safe to run repeatedly: values already written by the newest key
/// are left alone, so a second run does nothing.
///
/// A value that cannot be decrypted is reported and skipped, never
/// overwritten. The usual cause is a key that was removed from the
/// config while values it wrote were still stored - and the one thing
/// that must not happen then is this command replacing them with
/// garbage it could not read.
#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize, Resolve)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[empty_traits(KomodoWriteRequest)]
#[response(ReencryptSecretsResponse)]
#[error(mogh_error::Error)]
pub struct ReencryptSecrets {
  /// Report what would change without writing anything.
  #[serde(default)]
  pub dry_run: bool,
}

#[typeshare]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ReencryptSecretsResponse {
  /// Secret Variables rewritten with the newest key.
  pub variables: I64,
  /// Git provider account tokens rewritten.
  pub git_accounts: I64,
  /// Image registry account tokens rewritten.
  pub registry_accounts: I64,
  /// Values already written by the newest key, so left alone.
  pub already_current: I64,
  /// Values that could not be decrypted, by name. These were NOT
  /// written to. Almost always a key missing from `secret_keys`.
  pub failed: Vec<String>,
  /// True when nothing was written because `dry_run` was set.
  pub dry_run: bool,
}
