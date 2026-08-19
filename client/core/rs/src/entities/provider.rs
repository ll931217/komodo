use partial_derive2::Partial;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use super::MongoId;

#[typeshare(serialized_as = "Partial<GitProviderAccount>")]
pub type _PartialGitProviderAccount = PartialGitProviderAccount;

/// Configuration to access private git repos from various git providers.
/// Note. Cannot create two accounts with the same domain and username.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default, Partial)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[partial_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[diff_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[partial(skip_serializing_none, from, diff)]
#[cfg_attr(
  feature = "mongo",
  derive(mongo_indexed::derive::MongoIndexed)
)]
#[cfg_attr(feature = "mongo", unique_doc_index({ "domain": 1, "username": 1 }))]
pub struct GitProviderAccount {
  /// The Mongo ID of the git provider account.
  /// This field is de/serialized from/to JSON as
  /// `{ "_id": { "$oid": "..." }, ...(rest of serialized User) }`
  #[serde(
    default,
    rename = "_id",
    skip_serializing_if = "String::is_empty",
    with = "bson::serde_helpers::hex_string_as_object_id"
  )]
  pub id: MongoId,
  /// The domain of the provider.
  ///
  /// For git, this cannot include the protocol eg 'http://',
  /// which is controlled with 'https' field.
  #[cfg_attr(feature = "mongo", index)]
  #[serde(default = "default_git_domain")]
  #[partial_default(default_git_domain())]
  pub domain: String,
  /// Whether git provider is accessed over http or https.
  #[serde(default = "default_https")]
  #[partial_default(default_https())]
  pub https: bool,
  /// The account username
  #[cfg_attr(feature = "mongo", index)]
  #[serde(default)]
  pub username: String,
  /// The token in plain text on the db.
  /// If the database / host can be accessed this is insecure.
  #[serde(default)]
  pub token: String,
  /// Optional repo-path prefix this account covers, eg `my-group` or
  /// `my-group/subgroup`.
  ///
  /// When a resource names no git account, Komodo picks the account
  /// whose prefix is the longest segment-wise match for the repo path.
  /// Empty (the default, and every account created before this existed)
  /// means the account is only used when named explicitly, so leaving it
  /// unset preserves the previous behaviour exactly.
  ///
  /// Matching is on path segments: `infra` covers `infra/komodo` but NOT
  /// `infra-secrets/vault`.
  #[serde(default)]
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
}

fn default_git_domain() -> String {
  String::from("github.com")
}

fn default_https() -> bool {
  true
}

#[cfg(feature = "utoipa")]
impl utoipa::PartialSchema for PartialGitProviderAccount {
  fn schema()
  -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    utoipa::schema!(#[inline] std::collections::HashMap<String, serde_json::Value>).into()
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::ToSchema for PartialGitProviderAccount {}

#[typeshare(serialized_as = "Partial<ImageRegistryAccount>")]
pub type _PartialImageRegistryAccount = PartialImageRegistryAccount;

/// Configuration to access private image repositories on various registries.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default, Partial)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[partial_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[diff_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[partial(skip_serializing_none, from, diff)]
#[cfg_attr(
  feature = "mongo",
  derive(mongo_indexed::derive::MongoIndexed)
)]
#[cfg_attr(feature = "mongo", unique_doc_index({ "domain": 1, "username": 1 }))]
pub struct ImageRegistryAccount {
  /// The Mongo ID of the docker registry account.
  /// This field is de/serialized from/to JSON as
  /// `{ "_id": { "$oid": "..." }, ...(rest of ImageRegistryAccount) }`
  #[serde(
    default,
    rename = "_id",
    skip_serializing_if = "String::is_empty",
    with = "bson::serde_helpers::hex_string_as_object_id"
  )]
  pub id: MongoId,
  /// The domain of the provider.
  ///
  /// For docker registry, this can include 'http://...',
  /// however this is not recommended and won't work unless "insecure registries" are enabled
  /// on your hosts. See <https://docs.docker.com/reference/cli/dockerd/#insecure-registries>.
  #[cfg_attr(feature = "mongo", index)]
  #[serde(default = "default_registry_domain")]
  #[partial_default(default_registry_domain())]
  pub domain: String,
  /// The account username
  #[cfg_attr(feature = "mongo", index)]
  #[serde(default)]
  pub username: String,
  /// The token in plain text on the db.
  /// If the database / host can be accessed this is insecure.
  #[serde(default)]
  pub token: String,
}

fn default_registry_domain() -> String {
  String::from("docker.io")
}

#[cfg(feature = "utoipa")]
impl utoipa::PartialSchema for PartialImageRegistryAccount {
  fn schema()
  -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    utoipa::schema!(#[inline] std::collections::HashMap<String, serde_json::Value>).into()
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::ToSchema for PartialImageRegistryAccount {}
