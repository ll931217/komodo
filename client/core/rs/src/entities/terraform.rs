use bson::{Document, doc};
use derive_builder::Builder;
use derive_default_builder::DefaultBuilder;
use partial_derive2::Partial;
use serde::{Deserialize, Serialize};
use strum::Display;
use typeshare::typeshare;

use crate::deserializers::{
  env_vars_deserializer, file_contents_deserializer,
  option_env_vars_deserializer, option_file_contents_deserializer,
  option_string_list_deserializer, string_list_deserializer,
};

use super::resource::{Resource, ResourceListItem, ResourceQuery};

#[typeshare]
pub type TerraformListItem = ResourceListItem<TerraformListItemInfo>;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct TerraformListItemInfo {
  /// The Server whose Periphery runs terraform for this resource.
  pub server_id: String,
  /// The unit directory within the tree, relative to its root.
  pub run_directory: String,
  /// Where the terraform tree comes from.
  pub source_kind: TerraformSourceKind,
  /// Derived from the most recent run, not from a probe.
  pub state: TerraformState,
}

/// The outcome of this resource's last terraform run.
///
/// Unlike a Cluster, there is no cheap reachability probe to poll:
/// asking terraform for the truth means running a plan, which is a
/// real execution with real cost. So state is whatever the last run
/// reported, and drift is found on a schedule (see the Procedure
/// schedule + alert wiring), never by a background loop.
#[typeshare]
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  PartialOrd,
  Ord,
  Default,
  Serialize,
  Deserialize,
  Display,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum TerraformState {
  /// The last run succeeded, and the last plan found no changes.
  Ok,
  /// The last plan found pending changes: real infrastructure no
  /// longer matches the configuration.
  Drifted,
  /// The last run exited nonzero.
  Failed,
  /// Never run.
  #[default]
  Unknown,
}

#[cfg(feature = "utoipa")]
#[derive(utoipa::ToSchema)]
#[schema(as = Terraform)]
pub struct TerraformSchema(
  #[allow(unused)] Resource<TerraformConfig, TerraformInfo>,
);

#[typeshare]
pub type Terraform = Resource<TerraformConfig, TerraformInfo>;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct TerraformInfo {
  /// The outcome of the last run, written by the execute APIs.
  ///
  /// Persisted on the resource rather than derived from the last
  /// Update: a plan that succeeds and a plan that finds drift are both
  /// `success: true`, so the Update alone cannot tell them apart.
  #[serde(default)]
  pub state: TerraformState,
}

#[typeshare(serialized_as = "Partial<TerraformConfig>")]
pub type _PartialTerraformConfig = PartialTerraformConfig;

#[typeshare]
#[derive(
  Debug, Clone, Default, Serialize, Deserialize, Builder, Partial,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[partial_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(
  feature = "schemars",
  partial_derive(schemars::JsonSchema)
)]
#[diff_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[partial(skip_serializing_none, from, diff)]
pub struct TerraformConfig {
  /// The Server whose Periphery runs the terraform commands
  /// for this resource.
  #[serde(default, alias = "server")]
  #[partial_attr(serde(alias = "server"))]
  #[cfg_attr(
    feature = "schemars",
    partial_attr(schemars(rename = "server"))
  )]
  #[builder(default)]
  pub server_id: String,

  /// Terraform managed in Komodo, written as `main.tf` into a
  /// persistent working directory on the Server.
  /// Supports `[[VARIABLE]]` interpolation.
  ///
  /// Used only when no other source is configured. Precedence:
  /// `files_on_host`, then `linked_repo`, then `repo`, then this.
  #[serde(default, deserialize_with = "file_contents_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_file_contents_deserializer"
  ))]
  #[builder(default)]
  pub file_contents: String,

  /// Source the terraform tree from files already on the Server.
  #[serde(default)]
  #[builder(default)]
  pub files_on_host: bool,

  /// Directory on the Server holding the terraform tree.
  /// Required by `files_on_host`, ignored by every other source.
  #[serde(default)]
  #[builder(default)]
  pub root_directory: String,

  /// Choose a Komodo Repo (Resource) to source the tree.
  #[serde(default)]
  #[builder(default)]
  pub linked_repo: String,

  /// The git provider domain. Default: github.com
  #[serde(default = "default_git_provider")]
  #[builder(default = "default_git_provider()")]
  #[partial_default(default_git_provider())]
  pub git_provider: String,

  /// Whether to use https to clone the repo (versus http).
  #[serde(default = "default_git_https")]
  #[builder(default = "default_git_https()")]
  #[partial_default(default_git_https())]
  pub git_https: bool,

  /// The git account used to access private repos.
  /// Empty string can only clone public repos.
  #[serde(default)]
  #[builder(default)]
  pub git_account: String,

  /// The repo to source the tree from: {namespace}/{repo_name}
  #[serde(default)]
  #[builder(default)]
  pub repo: String,

  /// The branch of the repo. Default: main
  #[serde(default = "default_branch")]
  #[builder(default = "default_branch()")]
  #[partial_default(default_branch())]
  pub branch: String,

  /// Optionally pin a specific commit hash.
  #[serde(default)]
  #[builder(default)]
  pub commit: String,

  /// Optionally set an alternate clone path on the Server.
  #[serde(default)]
  #[builder(default)]
  pub clone_path: String,

  /// Delete and reclone the repo instead of pulling it.
  ///
  /// Safe with `managed_state`, which keeps the state file outside
  /// the checkout — a reclone would otherwise orphan real
  /// infrastructure by deleting its state.
  #[serde(default)]
  #[builder(default)]
  pub reclone: bool,

  /// The unit directory to run terraform in (`-chdir`), relative to
  /// the tree root. Empty runs the tree root itself.
  ///
  /// The WHOLE tree is always materialized, never just this
  /// directory: units reference `../../modules`-style relative paths,
  /// and terraform refuses a module path escaping the tree it was
  /// given.
  #[serde(default)]
  #[builder(default)]
  pub run_directory: String,

  /// Environment written to a private env file on the Server and
  /// sourced before the run — `TF_VAR_*`, provider credentials.
  /// Supports `[[VARIABLE]]` interpolation.
  ///
  /// Never passed on the command line, where it would be visible to
  /// any other process on the host.
  #[serde(default, deserialize_with = "env_vars_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_env_vars_deserializer"
  ))]
  #[builder(default)]
  pub environment: String,

  /// Whether to skip interpolating Komodo Variables / secrets into
  /// `environment` and `file_contents`.
  #[serde(default)]
  #[builder(default)]
  pub skip_secret_interp: bool,

  /// Keep the local backend's state file outside the checkout, at a
  /// Periphery-managed path, via `init -backend-config=path=`.
  ///
  /// Default true. Turn it off for units that declare their own
  /// remote backend (S3, GitLab http, ...), where redirecting the
  /// local backend would be wrong.
  #[serde(default = "default_managed_state")]
  #[builder(default = "default_managed_state()")]
  #[partial_default(default_managed_state())]
  pub managed_state: bool,

  /// Optionally bridge a Komodo Cluster: its kubeconfig is
  /// materialized as a private temp file for the duration of the run
  /// and exported as `TF_VAR_kubeconfig_path` / `KUBE_CONFIG_PATH`,
  /// so the kubernetes and helm providers can authenticate without a
  /// second copy of the credentials.
  #[serde(default, alias = "cluster")]
  #[partial_attr(serde(alias = "cluster"))]
  #[cfg_attr(
    feature = "schemars",
    partial_attr(schemars(rename = "cluster"))
  )]
  #[builder(default)]
  pub cluster_id: String,

  /// Proxy exported as HTTP_PROXY / HTTPS_PROXY for providers that
  /// fetch from outside the cluster, such as helm chart repos.
  #[serde(default)]
  #[builder(default)]
  pub proxy_url: String,

  /// NO_PROXY value exported alongside `proxy_url`, so the Kubernetes
  /// api server is dialed directly instead of through the proxy.
  #[serde(default)]
  #[builder(default)]
  pub no_proxy: String,

  /// Additional arguments passed to plan / apply / destroy.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub extra_args: Vec<String>,

  /// Whether to alert when a scheduled plan finds drift,
  /// or when a run fails.
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_alerts: bool,

  /// Whether incoming webhooks trigger a plan for this resource.
  #[serde(default = "default_webhook_enabled")]
  #[builder(default = "default_webhook_enabled()")]
  #[partial_default(default_webhook_enabled())]
  pub webhook_enabled: bool,

  /// An alternate webhook secret for this resource.
  /// Empty uses the default secret from the core config.
  #[serde(default)]
  #[builder(default)]
  pub webhook_secret: String,

  /// Configure quick links that are displayed in the resource header
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub links: Vec<String>,
}

impl TerraformConfig {
  pub fn env_vars(
    &self,
  ) -> anyhow::Result<Vec<super::EnvironmentVar>> {
    anyhow::Context::context(
      super::environment_vars_from_str(&self.environment),
      "Invalid environment",
    )
  }

  /// Which source this resource's terraform tree comes from.
  ///
  /// Only one applies, so the order is fixed rather than left to
  /// whichever field happens to be set: host files, then a linked
  /// Repo, then an inline repo, then contents managed here.
  pub fn source_kind(&self) -> TerraformSourceKind {
    if self.files_on_host {
      TerraformSourceKind::FilesOnHost
    } else if !self.linked_repo.is_empty() {
      TerraformSourceKind::LinkedRepo
    } else if !self.repo.is_empty() {
      TerraformSourceKind::Repo
    } else {
      TerraformSourceKind::Contents
    }
  }
}

/// Where a Terraform resource's tree comes from.
#[typeshare]
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  Serialize,
  Deserialize,
  Display,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum TerraformSourceKind {
  /// A tree already present on the Server.
  FilesOnHost,
  /// A Komodo Repo resource.
  LinkedRepo,
  /// A git repo configured on the Terraform resource itself.
  Repo,
  /// Terraform managed in Komodo.
  #[default]
  Contents,
}

impl From<&Terraform> for crate::entities::RepoExecutionArgs {
  fn from(terraform: &Terraform) -> Self {
    Self {
      name: terraform.name.clone(),
      provider: terraform.config.git_provider.clone(),
      https: terraform.config.git_https,
      account: crate::entities::optional_string(
        &terraform.config.git_account,
      ),
      repo: crate::entities::optional_string(&terraform.config.repo),
      branch: terraform.config.branch.clone(),
      commit: crate::entities::optional_string(
        &terraform.config.commit,
      ),
      destination: None,
      default_folder: crate::entities::DefaultRepoFolder::Stacks,
    }
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::PartialSchema for PartialTerraformConfig {
  fn schema()
  -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    TerraformConfig::schema()
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::ToSchema for PartialTerraformConfig {}

/// Modeled on [StackActionState][super::stack::StackActionState],
/// deliberately not on `ClusterActionState`: every flag here gates a
/// real terraform invocation, and `busy()` is what stops two applies
/// racing on one working directory and one state file.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct TerraformActionState {
  pub initializing: bool,
  pub planning: bool,
  pub applying: bool,
  pub destroying: bool,
}

#[typeshare]
pub type TerraformQuery = ResourceQuery<TerraformQuerySpecifics>;

#[typeshare]
#[derive(
  Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum TerraformSortBy {
  /// Sort by name. Default.
  #[default]
  Name,
  /// Sort by state.
  State,
}

#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, Default, DefaultBuilder,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct TerraformQuerySpecifics {
  /// Filter by server ids.
  pub servers: Vec<String>,
  /// Filter by bridged Cluster ids.
  pub clusters: Vec<String>,
}

impl super::resource::AddFilters for TerraformQuerySpecifics {
  fn add_filters(&self, filters: &mut Document) {
    if !self.servers.is_empty() {
      filters
        .insert("config.server_id", doc! { "$in": &self.servers });
    }
    if !self.clusters.is_empty() {
      filters
        .insert("config.cluster_id", doc! { "$in": &self.clusters });
    }
  }
}

fn default_git_provider() -> String {
  String::from("github.com")
}

fn default_git_https() -> bool {
  true
}

fn default_branch() -> String {
  String::from("main")
}

fn default_managed_state() -> bool {
  true
}

fn default_send_alerts() -> bool {
  true
}

fn default_webhook_enabled() -> bool {
  true
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn source_precedence_is_fixed_not_field_order() {
    // Every source set at once: host files win, then linked repo,
    // then inline repo, then contents. A run that picks the wrong
    // one silently applies the wrong infrastructure.
    let mut config = TerraformConfig {
      files_on_host: true,
      linked_repo: String::from("repo-resource"),
      repo: String::from("namespace/repo"),
      file_contents: String::from("resource \"terraform_data\" {}"),
      ..Default::default()
    };
    assert_eq!(
      config.source_kind(),
      TerraformSourceKind::FilesOnHost
    );

    config.files_on_host = false;
    assert_eq!(config.source_kind(), TerraformSourceKind::LinkedRepo);

    config.linked_repo = String::new();
    assert_eq!(config.source_kind(), TerraformSourceKind::Repo);

    config.repo = String::new();
    assert_eq!(config.source_kind(), TerraformSourceKind::Contents);
  }
}
