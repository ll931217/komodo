use anyhow::{Context, anyhow};
use interpolate::Interpolator;
use komodo_client::entities::{
  EnvironmentVar,
  cluster::Cluster,
  repo::Repo,
  terraform::{Terraform, TerraformSourceKind},
};
use periphery_client::api::terraform::TerraformSource;

use super::{
  cluster::cluster_target_and_replacers,
  git_token,
  query::{VariablesAndSecrets, get_variables_and_secrets},
};

/// How a run authenticates to a bridged Kubernetes cluster.
#[derive(Default)]
pub struct TerraformKubeconfig {
  /// Materialized by Periphery as a private temp file for the run.
  pub contents: String,
  /// An existing path on the Server, used when the Cluster is
  /// configured that way rather than by contents.
  pub path: String,
  /// Replacers from interpolating the kubeconfig, to be merged into
  /// the run's own set - a kubeconfig is credentials, and Update logs
  /// must not carry them.
  pub secret_replacers: Vec<(String, String)>,
}

/// Resolve the optional `cluster_id` bridge.
///
/// Komodo already holds the cluster credentials the kubernetes and helm
/// providers need, so a workloads-shape unit points at a Cluster
/// instead of carrying a second copy. Empty when no Cluster is
/// attached.
pub async fn terraform_kubeconfig(
  terraform: &Terraform,
) -> anyhow::Result<TerraformKubeconfig> {
  if terraform.config.cluster_id.is_empty() {
    return Ok(Default::default());
  }
  let cluster =
    crate::resource::get::<Cluster>(&terraform.config.cluster_id)
      .await
      .context("Failed to get the Terraform's bridged Cluster")?;
  let (target, secret_replacers) =
    cluster_target_and_replacers(&cluster).await?;
  if target.kubeconfig_contents.is_empty()
    && target.kubeconfig_path.is_empty()
  {
    return Err(anyhow!(
      "Bridged Cluster '{}' has no kubeconfig configured",
      cluster.name
    ));
  }
  Ok(TerraformKubeconfig {
    contents: target.kubeconfig_contents,
    path: target.kubeconfig_path,
    secret_replacers,
  })
}

/// A Terraform resource's run inputs, with Variables / secrets already
/// interpolated.
pub struct InterpolatedTerraform {
  /// Root config managed in Komodo, interpolated.
  pub file_contents: String,
  /// `TF_VAR_*` and provider credentials, interpolated.
  pub environment: Vec<EnvironmentVar>,
  /// (secret value, replacement) pairs, so command output can be
  /// scrubbed before it is stored in an Update or shown to a user.
  pub secret_replacers: Vec<(String, String)>,
}

/// Interpolate a Terraform resource's contents and environment in a
/// single pass.
///
/// Done on Core so Periphery never has to resolve Komodo Variables.
/// One interpolator covers both strings, so the returned replacers
/// scrub secrets that came from either.
pub async fn interpolated_terraform(
  terraform: &Terraform,
) -> anyhow::Result<InterpolatedTerraform> {
  let mut file_contents = terraform.config.file_contents.clone();
  let mut environment = terraform.config.environment.clone();
  let mut secret_replacers = Vec::new();

  if !terraform.config.skip_secret_interp {
    let VariablesAndSecrets { variables, secrets } =
      get_variables_and_secrets()
        .await
        .context("Failed to get variables and secrets")?;
    let mut interpolator =
      Interpolator::new(Some(&variables), &secrets);
    interpolator
      .interpolate_string(&mut file_contents)
      .context("Failed to interpolate variables into file contents")?
      .interpolate_string(&mut environment)
      .context("Failed to interpolate variables into environment")?;
    secret_replacers =
      interpolator.secret_replacers.into_iter().collect();
  }

  let mut config = terraform.config.clone();
  config.environment = environment;

  Ok(InterpolatedTerraform {
    file_contents,
    environment: config.env_vars()?,
    secret_replacers,
  })
}

/// Resolve where a Terraform resource's tree comes from into the flat
/// spec Periphery understands.
///
/// A linked Komodo Repo is flattened here, so Periphery never has to
/// know Repo resources exist - the same split the Cluster uses.
pub async fn terraform_source(
  terraform: &Terraform,
  file_contents: String,
) -> anyhow::Result<TerraformSource> {
  match terraform.config.source_kind() {
    TerraformSourceKind::Contents => {
      if file_contents.trim().is_empty() {
        return Err(anyhow!(
          "Terraform has no source configured: set 'files_on_host', 'linked_repo', 'repo', or 'file_contents'"
        ));
      }
      Ok(TerraformSource::Contents(file_contents))
    }

    TerraformSourceKind::FilesOnHost => {
      if terraform.config.root_directory.is_empty() {
        return Err(anyhow!(
          "'files_on_host' needs a 'root_directory' holding the terraform tree"
        ));
      }
      Ok(TerraformSource::FilesOnHost {
        root_directory: terraform.config.root_directory.clone(),
      })
    }

    TerraformSourceKind::LinkedRepo => {
      let mut repo =
        crate::resource::get::<Repo>(&terraform.config.linked_repo)
          .await
          .context("Failed to get the Terraform's linked Repo")?;
      let git_token = git_token(
        &repo.config.git_provider,
        &repo.config.git_account,
        |https| repo.config.git_https = https,
      )
      .await
      .with_context(|| {
        format!(
          "Failed to get git token for the linked Repo | {} | {}",
          repo.config.git_provider, repo.config.git_account
        )
      })?;
      Ok(TerraformSource::Repo {
        args: (&repo).into(),
        git_token,
        reclone: terraform.config.reclone,
      })
    }

    TerraformSourceKind::Repo => {
      let mut terraform = terraform.clone();
      let git_token = git_token(
        &terraform.config.git_provider,
        &terraform.config.git_account,
        |https| terraform.config.git_https = https,
      )
      .await
      .with_context(|| {
        format!(
          "Failed to get git token | {} | {}",
          terraform.config.git_provider, terraform.config.git_account
        )
      })?;
      Ok(TerraformSource::Repo {
        args: (&terraform).into(),
        git_token,
        reclone: terraform.config.reclone,
      })
    }
  }
}
