use anyhow::{Context, anyhow};
use interpolate::Interpolator;
use komodo_client::entities::{
  application::{Application, ApplicationSourceKind},
  cluster::Cluster,
  repo::Repo,
};
use periphery_client::api::cluster::ClusterManifestSource;

use super::{
  git_token,
  query::{VariablesAndSecrets, get_variables_and_secrets},
};

/// An Application's manifests, with Variables / secrets interpolated.
pub struct InterpolatedApplication {
  /// The manifests to apply, interpolated.
  pub manifests: String,
  /// (secret value, replacement) pairs, so command output can be
  /// scrubbed before it is stored in an Update or shown to a user.
  pub secret_replacers: Vec<(String, String)>,
}

/// Interpolate an Application's manifests.
///
/// Done on Core so Periphery never has to resolve Komodo Variables.
/// The Cluster's kubeconfig is interpolated separately, by
/// [interpolated_cluster][super::cluster::interpolated_cluster] - the
/// two resources carry their own `skip_secret_interp`, and a Cluster
/// shared by ten Applications should not have its setting decided by
/// whichever one is deploying.
pub async fn interpolated_application(
  application: &Application,
) -> anyhow::Result<InterpolatedApplication> {
  let mut manifests = application.config.file_contents.clone();
  let mut secret_replacers = Vec::new();

  if !application.config.skip_secret_interp {
    let VariablesAndSecrets { variables, secrets } =
      get_variables_and_secrets()
        .await
        .context("Failed to get variables and secrets")?;
    let mut interpolator =
      Interpolator::new(Some(&variables), &secrets);
    interpolator
      .interpolate_string(&mut manifests)
      .context("Failed to interpolate variables into manifests")?;
    secret_replacers =
      interpolator.secret_replacers.into_iter().collect();
  }

  Ok(InterpolatedApplication {
    manifests,
    secret_replacers,
  })
}

/// Resolve where an Application's manifests come from into the flat
/// spec Periphery understands.
///
/// A linked Komodo Repo is flattened here, so Periphery never has to
/// know Repo resources exist - the same split the Cluster uses for its
/// kubeconfig.
pub async fn application_manifest_source(
  application: &Application,
  manifests: String,
) -> anyhow::Result<ClusterManifestSource> {
  match application.config.manifest_source() {
    ApplicationSourceKind::Contents => {
      Ok(ClusterManifestSource::Contents(manifests))
    }

    ApplicationSourceKind::FilesOnHost => {
      if application.config.run_directory.is_empty() {
        return Err(anyhow!(
          "'files_on_host' needs a 'run_directory' to read from"
        ));
      }
      Ok(ClusterManifestSource::FilesOnHost {
        run_directory: application.config.run_directory.clone(),
        file_paths: application.config.file_paths.clone(),
      })
    }

    ApplicationSourceKind::LinkedRepo => {
      let mut repo =
        crate::resource::get::<Repo>(&application.config.linked_repo)
          .await
          .context("Failed to get the Application's linked Repo")?;
      let git_token = git_token(
        &repo.config.git_provider,
        &repo.config.git_account,
        Some(&repo.config.repo),
        |https| repo.config.git_https = https,
      )
      .await
      .with_context(|| {
        format!(
          "Failed to get git token for the linked Repo | {} | {}",
          repo.config.git_provider, repo.config.git_account
        )
      })?;
      Ok(ClusterManifestSource::Repo {
        args: (&repo).into(),
        git_token,
        reclone: application.config.reclone,
        // The Application says where in the repo its manifests live;
        // the Repo resource only says how to fetch it.
        run_directory: application.config.run_directory.clone(),
        file_paths: application.config.file_paths.clone(),
      })
    }

    ApplicationSourceKind::Repo => {
      let mut application = application.clone();
      let git_token = git_token(
        &application.config.git_provider,
        &application.config.git_account,
        Some(&application.config.repo),
        |https| application.config.git_https = https,
      )
      .await
      .with_context(|| {
        format!(
          "Failed to get git token | {} | {}",
          application.config.git_provider,
          application.config.git_account
        )
      })?;
      Ok(ClusterManifestSource::Repo {
        args: (&application).into(),
        git_token,
        reclone: application.config.reclone,
        run_directory: application.config.run_directory.clone(),
        file_paths: application.config.file_paths.clone(),
      })
    }
  }
}

/// The Cluster an Application deploys to.
///
/// Attaching it was permission-checked at create/update, so this is a
/// plain fetch - the same treatment a Cluster gives its Server.
pub async fn application_cluster(
  application: &Application,
) -> anyhow::Result<Cluster> {
  if application.config.cluster_id.is_empty() {
    return Err(anyhow!(
      "Application has no Cluster configured: nothing to deploy to"
    ));
  }
  crate::resource::get::<Cluster>(&application.config.cluster_id)
    .await
    .context("Failed to get the Application's Cluster")
}

/// An Application's helm values, with Variables / secrets
/// interpolated, and the release name defaulted to the Application's
/// own name.
///
/// The chart reference and `--set` arguments are interpolated too: a
/// registry path or an image tag is exactly the kind of thing a
/// Variable holds.
pub async fn interpolated_helm(
  application: &Application,
  secret_replacers: &mut Vec<(String, String)>,
) -> anyhow::Result<komodo_client::entities::application::HelmSource>
{
  let mut helm = application.config.helm.clone();
  if helm.is_none() {
    return Ok(helm);
  }
  if helm.release_name.trim().is_empty() {
    helm.release_name = application.name.clone();
  }
  if application.config.skip_secret_interp {
    return Ok(helm);
  }
  let VariablesAndSecrets { variables, secrets } =
    get_variables_and_secrets()
      .await
      .context("Failed to get variables and secrets")?;
  let mut interpolator =
    Interpolator::new(Some(&variables), &secrets);
  interpolator
    .interpolate_string(&mut helm.values)?
    .interpolate_string(&mut helm.chart)?
    .interpolate_extra_args(&mut helm.set)?
    .interpolate_extra_args(&mut helm.extra_args)?;
  secret_replacers.extend(interpolator.secret_replacers);
  Ok(helm)
}
