use bson::{Document, doc};
use derive_builder::Builder;
use derive_default_builder::DefaultBuilder;
use partial_derive2::Partial;
use serde::{Deserialize, Serialize};
use strum::Display;
use typeshare::typeshare;

use crate::deserializers::{
  file_contents_deserializer, option_file_contents_deserializer,
  option_string_list_deserializer, string_list_deserializer,
};

use super::resource::{Resource, ResourceListItem, ResourceQuery};

#[typeshare]
pub type ApplicationListItem =
  ResourceListItem<ApplicationListItemInfo>;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ApplicationListItemInfo {
  /// The Cluster this Application deploys to.
  pub cluster_id: String,
  /// The namespace it deploys into.
  pub namespace: String,
  /// Where the manifests come from.
  pub source_kind: ApplicationSourceKind,
  /// Derived from the most recent execution, not from a probe.
  pub state: ApplicationState,
}

/// The outcome of this Application's last execution.
///
/// There is no probe behind this. A Cluster's reachability is cheap to
/// poll; whether an Application's manifests still match what is running
/// is not - answering that means a `kubectl diff`, which is a real
/// execution. So this is whatever the last run reported, and drift is
/// found by a scheduled Diff (see the Procedure schedule wiring),
/// never by a background loop.
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
pub enum ApplicationState {
  /// The last Deploy succeeded, or the last Diff found no differences.
  Deployed,
  /// The last Diff found differences between the manifests and the
  /// cluster. Not a failure: the diff itself succeeded, and what it
  /// reports is that reality has moved.
  Drifted,
  /// The last execution failed.
  Failed,
  /// Never deployed, or destroyed since.
  #[default]
  Unknown,
}

#[cfg(feature = "utoipa")]
#[derive(utoipa::ToSchema)]
#[schema(as = Application)]
pub struct ApplicationSchema(
  #[allow(unused)] Resource<ApplicationConfig, ApplicationInfo>,
);

#[typeshare]
pub type Application = Resource<ApplicationConfig, ApplicationInfo>;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ApplicationInfo {
  /// The outcome of the last execution, written by the execute APIs.
  #[serde(default)]
  pub state: ApplicationState,
}

#[typeshare(serialized_as = "Partial<ApplicationConfig>")]
pub type _PartialApplicationConfig = PartialApplicationConfig;

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
pub struct ApplicationConfig {
  /// The Cluster this Application deploys to.
  ///
  /// Exactly one: a second environment is a second Application, which
  /// is also where the namespace and any per-environment values
  /// differ. The Cluster supplies the kubeconfig, the Server whose
  /// Periphery runs kubectl, and the policy this Application cannot
  /// widen (allowed namespaces, whether cluster-scoped objects may be
  /// touched at all).
  #[serde(default, alias = "cluster")]
  #[partial_attr(serde(alias = "cluster"))]
  #[cfg_attr(
    feature = "schemars",
    partial_attr(schemars(rename = "cluster"))
  )]
  #[builder(default)]
  pub cluster_id: String,

  /// The namespace to deploy into.
  /// Empty uses the Cluster's default namespace.
  ///
  /// Must be permitted by the Cluster's allowed namespaces. Note that
  /// a kustomization setting `namespace:` itself wins over this field
  /// for the objects it generates - this is what Komodo passes to
  /// kubectl, not a guarantee about what the manifests declare.
  #[serde(default)]
  #[builder(default)]
  pub namespace: String,

  /// Manifests managed in Komodo, written to the Server at execution
  /// time. Supports `[[VARIABLE]]` interpolation.
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

  /// Source the manifests from files already on the Server.
  #[serde(default)]
  #[builder(default)]
  pub files_on_host: bool,

  /// Choose a Komodo Repo (Resource) to source the manifests.
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

  /// The repo to source manifests from: {namespace}/{repo_name}
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
  #[serde(default)]
  #[builder(default)]
  pub reclone: bool,

  /// The directory the manifests live in, relative to the repo root or
  /// to the host filesystem root for `files_on_host`.
  #[serde(default)]
  #[builder(default)]
  pub run_directory: String,

  /// Manifest paths relative to `run_directory`.
  /// Empty applies the whole directory.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub file_paths: Vec<String>,

  /// Apply with kustomize (`kubectl apply -k`) instead of treating the
  /// manifests as plain resource files. Requires a `kustomization.yaml`
  /// in the run directory; `file_paths` is ignored when this is on.
  #[serde(default)]
  #[builder(default)]
  pub kustomize: bool,

  /// Whether to skip interpolating Komodo Variables / secrets into
  /// the manifests.
  #[serde(default)]
  #[builder(default)]
  pub skip_secret_interp: bool,

  /// After a successful apply, wait for the applied workloads to roll
  /// out (`kubectl rollout status`) and fail the Deploy if they never
  /// become ready. Without it a Deploy succeeds as soon as the api
  /// server accepts the manifests, even if every pod crashloops.
  #[serde(default)]
  #[builder(default)]
  pub wait_ready: bool,

  /// Additional arguments passed to `kubectl apply` / `kubectl delete`.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub extra_args: Vec<String>,

  /// Whether to alert when a scheduled Diff finds differences,
  /// or when a Deploy fails.
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_alerts: bool,

  /// Whether incoming webhooks trigger a Deploy for this Application.
  #[serde(default = "default_webhook_enabled")]
  #[builder(default = "default_webhook_enabled()")]
  #[partial_default(default_webhook_enabled())]
  pub webhook_enabled: bool,

  /// An alternate webhook secret for this Application.
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

impl ApplicationConfig {
  /// Which manifest source this Application uses.
  ///
  /// Only one applies, so the order is fixed rather than left to
  /// whichever field happens to be set: host files, then a linked
  /// Repo, then an inline repo, then contents managed here.
  pub fn manifest_source(&self) -> ApplicationSourceKind {
    if self.files_on_host {
      ApplicationSourceKind::FilesOnHost
    } else if !self.linked_repo.is_empty() {
      ApplicationSourceKind::LinkedRepo
    } else if !self.repo.is_empty() {
      ApplicationSourceKind::Repo
    } else {
      ApplicationSourceKind::Contents
    }
  }
}

/// Where an Application's manifests come from.
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
pub enum ApplicationSourceKind {
  /// Manifests already present on the Server.
  FilesOnHost,
  /// A Komodo Repo resource.
  LinkedRepo,
  /// A git repo configured on the Application itself.
  Repo,
  /// Manifests managed in Komodo.
  #[default]
  Contents,
}

impl From<&Application> for crate::entities::RepoExecutionArgs {
  fn from(application: &Application) -> Self {
    Self {
      name: application.name.clone(),
      provider: application.config.git_provider.clone(),
      https: application.config.git_https,
      account: crate::entities::optional_string(
        &application.config.git_account,
      ),
      repo: crate::entities::optional_string(
        &application.config.repo,
      ),
      branch: application.config.branch.clone(),
      commit: crate::entities::optional_string(
        &application.config.commit,
      ),
      destination: None,
      default_folder: crate::entities::DefaultRepoFolder::Stacks,
      ssh: None,
    }
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::PartialSchema for PartialApplicationConfig {
  fn schema()
  -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    ApplicationConfig::schema()
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::ToSchema for PartialApplicationConfig {}

/// One flag per execute op, so `busy()` rejects a second execution
/// while one is in flight: apply, delete and diff share a manifest
/// clone directory on the Server, and two at once corrupt each other's
/// checkout even when they target different namespaces.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ApplicationActionState {
  pub deploying: bool,
  pub destroying: bool,
  pub diffing: bool,
}

#[typeshare]
pub type ApplicationQuery = ResourceQuery<ApplicationQuerySpecifics>;

#[typeshare]
#[derive(
  Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum ApplicationSortBy {
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
pub struct ApplicationQuerySpecifics {
  /// Filter by Cluster ids.
  pub clusters: Vec<String>,
}

impl super::resource::AddFilters for ApplicationQuerySpecifics {
  fn add_filters(&self, filters: &mut Document) {
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
    // then inline repo, then contents. Picking the wrong one silently
    // deploys the wrong manifests.
    let mut config = ApplicationConfig {
      files_on_host: true,
      linked_repo: String::from("repo-resource"),
      repo: String::from("namespace/repo"),
      file_contents: String::from("kind: ConfigMap\n"),
      ..Default::default()
    };
    assert_eq!(
      config.manifest_source(),
      ApplicationSourceKind::FilesOnHost
    );

    config.files_on_host = false;
    assert_eq!(
      config.manifest_source(),
      ApplicationSourceKind::LinkedRepo
    );

    config.linked_repo = String::new();
    assert_eq!(config.manifest_source(), ApplicationSourceKind::Repo);

    config.repo = String::new();
    assert_eq!(
      config.manifest_source(),
      ApplicationSourceKind::Contents
    );
  }
}
