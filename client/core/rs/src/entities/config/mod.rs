use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use typeshare::typeshare;

#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "core")]
pub mod core;
#[cfg(feature = "periphery")]
pub mod periphery;

#[cfg(any(feature = "core", feature = "periphery"))]
fn default_config_keywords() -> Vec<String> {
  vec![String::from("*config.*")]
}

#[cfg(any(feature = "cli", feature = "core", feature = "periphery"))]
fn default_merge_nested_config() -> bool {
  true
}

#[cfg(any(feature = "cli", feature = "core", feature = "periphery"))]
fn default_extend_config_arrays() -> bool {
  true
}

/// Provide database connection information.
/// Komodo uses the MongoDB api driver for database communication,
/// and FerretDB to support Postgres and Sqlite storage options.
///
/// Must provide ONE of:
/// 1. `uri`
/// 2. `address` + `username` + `password`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatabaseConfig {
  /// Full mongo uri string, eg. `mongodb://username:password@your.mongo.int:27017`
  #[serde(default, skip_serializing_if = "String::is_empty")]
  pub uri: String,
  /// Just the address part of the mongo uri, eg `your.mongo.int:27017`
  #[serde(
    default = "default_database_address",
    skip_serializing_if = "String::is_empty"
  )]
  pub address: String,
  /// Mongo user username
  #[serde(default, skip_serializing_if = "String::is_empty")]
  pub username: String,
  /// Mongo user password
  #[serde(default, skip_serializing_if = "String::is_empty")]
  pub password: String,
  /// Mongo app name. default: `komodo_core`
  #[serde(default = "default_database_app_name")]
  pub app_name: String,
  /// Mongo db name. Which mongo database to create the collections in.
  /// Default: `komodo`.
  #[serde(default = "default_database_db_name")]
  pub db_name: String,
}

fn default_database_address() -> String {
  String::from("localhost:27017")
}

fn default_database_app_name() -> String {
  "komodo_core".to_string()
}

fn default_database_db_name() -> String {
  "komodo".to_string()
}

impl Default for DatabaseConfig {
  fn default() -> Self {
    Self {
      uri: Default::default(),
      address: default_database_address(),
      username: Default::default(),
      password: Default::default(),
      app_name: default_database_app_name(),
      db_name: default_database_db_name(),
    }
  }
}

fn default_database_config() -> &'static DatabaseConfig {
  static DEFAULT_DATABASE_CONFIG: OnceLock<DatabaseConfig> =
    OnceLock::new();
  DEFAULT_DATABASE_CONFIG.get_or_init(Default::default)
}

impl DatabaseConfig {
  pub fn sanitized(&self) -> DatabaseConfig {
    DatabaseConfig {
      uri: empty_or_redacted(&self.uri),
      address: self.address.clone(),
      username: empty_or_redacted(&self.username),
      password: empty_or_redacted(&self.password),
      app_name: self.app_name.clone(),
      db_name: self.db_name.clone(),
    }
  }

  pub fn is_default(&self) -> bool {
    self == default_database_config()
  }
}

#[typeshare]
#[derive(
  Debug,
  Clone,
  PartialEq,
  Eq,
  Hash,
  PartialOrd,
  Ord,
  Serialize,
  Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct GitProvider {
  /// The git provider domain. Default: `github.com`.
  #[serde(default = "default_git_provider")]
  pub domain: String,
  /// Whether to use https. Default: true.
  #[serde(default = "default_git_https")]
  pub https: bool,
  /// The accounts on the git provider. Required.
  #[serde(alias = "account")]
  pub accounts: Vec<ProviderAccount>,
}

fn default_git_provider() -> String {
  String::from("github.com")
}

fn default_git_https() -> bool {
  true
}

#[typeshare]
#[derive(
  Debug,
  Clone,
  PartialEq,
  Eq,
  Hash,
  PartialOrd,
  Ord,
  Serialize,
  Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ImageRegistry {
  /// The image provider domain. Default: `docker.io`.
  #[serde(default = "default_image_provider")]
  pub domain: String,
  /// The accounts on the registry. Required.
  #[serde(alias = "account")]
  pub accounts: Vec<ProviderAccount>,
  /// Available organizations on the registry provider.
  /// Used to push an image under an organization's repo rather than an account's repo.
  #[serde(default, alias = "organization")]
  pub organizations: Vec<String>,
}

fn default_image_provider() -> String {
  String::from("docker.io")
}

#[typeshare]
#[derive(
  Debug,
  Clone,
  PartialEq,
  Eq,
  Hash,
  PartialOrd,
  Ord,
  Serialize,
  Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ProviderAccount {
  /// The account username. Required.
  #[serde(alias = "account")]
  pub username: String,
  /// The account access token. Required.
  #[serde(default, skip_serializing)]
  pub token: String,
  /// Optional repo-path prefix this account covers, eg `my-group` or
  /// `my-group/subgroup`.
  ///
  /// When a resource names no git account, Komodo picks the account
  /// whose prefix is the longest segment-wise match for the repo path.
  /// Empty (the default, and every existing account) means the account
  /// is only used when named explicitly, so setting nothing preserves
  /// today's behaviour exactly.
  ///
  /// Matching is on path segments: a prefix of `infra` covers
  /// `infra/komodo` but NOT `infra-secrets/vault`.
  #[serde(default, alias = "prefix")]
  pub path_prefix: String,
  /// Optional SSH private key, in OpenSSH format, for reaching this
  /// provider over ssh instead of http(s).
  ///
  /// Setting this is what switches the remote to ssh - there is no
  /// separate toggle, because two settings that must agree is a state
  /// you can get wrong, and "I gave it an ssh key but it still used
  /// https" is a confusing way to fail.
  ///
  /// The key is written to a private per-operation file for the duration
  /// of a git command and removed afterwards. It never appears in a
  /// command line or a log.
  #[serde(default)]
  pub ssh_private_key: String,
  /// known_hosts entries for this provider, one per line, in the format
  /// `ssh-keyscan` emits.
  ///
  /// Required for ssh unless `ssh_accept_new_host_keys` is set: without
  /// a known host key there is nothing to verify the remote against.
  #[serde(default)]
  pub ssh_known_hosts: String,
  /// Trust the remote's host key on first contact instead of requiring
  /// it in `ssh_known_hosts`.
  ///
  /// Weaker - a man-in-the-middle at first contact is not detected - but
  /// it is what most tooling does by default, so it is offered as an
  /// explicit opt-in rather than being chosen on your behalf.
  /// Verification is never disabled entirely.
  #[serde(default)]
  pub ssh_accept_new_host_keys: bool,
  /// Optional PEM client certificate, for a git host that authenticates
  /// clients by certificate rather than token.
  ///
  /// Must be paired with `tls_client_key`; half a credential cannot
  /// authenticate and git reports it as an opaque TLS error.
  #[serde(default)]
  pub tls_client_cert: String,
  /// The PEM private key for `tls_client_cert`.
  ///
  /// Written to a private per-operation file for the duration of a git
  /// command. Only its path reaches the command line.
  #[serde(default)]
  pub tls_client_key: String,
  /// Optional PEM CA bundle to trust for THIS provider, for a
  /// self-signed or internal-CA git host.
  ///
  /// Scoped to the invocation, so trusting an internal CA here never
  /// weakens verification for any other remote.
  #[serde(default)]
  pub tls_ca_bundle: String,
  /// GitHub App id, for authenticating as an App installation instead
  /// of with a long-lived personal access token.
  ///
  /// Requires `github_app_installation_id` and
  /// `github_app_private_key`. When set, the `token` field is ignored:
  /// Komodo mints a fresh installation token, which expires in an hour.
  #[serde(default)]
  pub github_app_id: String,
  /// The installation id for `github_app_id` - the specific org or user
  /// that installed the App.
  #[serde(default)]
  pub github_app_installation_id: String,
  /// The App's RSA private key, in the PKCS#1 PEM GitHub issues
  /// (`BEGIN RSA PRIVATE KEY`).
  ///
  /// Never leaves Core: it signs a short-lived JWT which is exchanged
  /// for the installation token that actually reaches git.
  #[serde(default)]
  pub github_app_private_key: String,
}

pub fn empty_or_redacted(src: &str) -> String {
  if src.is_empty() {
    String::new()
  } else {
    String::from("##############")
  }
}
