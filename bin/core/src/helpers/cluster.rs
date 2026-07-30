use anyhow::{Context, anyhow};
use interpolate::Interpolator;
use komodo_client::entities::{
  cluster::{Cluster, ClusterManifestSourceKind},
  repo::Repo,
};
use periphery_client::api::cluster::{
  ClusterManifestSource, ClusterTarget,
};

use super::{
  git_token,
  query::{VariablesAndSecrets, get_variables_and_secrets},
};

/// A Cluster's connection target and manifests, with Variables /
/// secrets already interpolated.
pub struct InterpolatedCluster {
  /// What Periphery needs to reach the cluster.
  pub target: ClusterTarget,
  /// The manifests to apply, interpolated.
  pub manifests: String,
  /// (secret value, replacement) pairs, so command output can be
  /// scrubbed before it is stored in an Update or shown to a user.
  pub secret_replacers: Vec<(String, String)>,
}

/// Interpolate a Cluster's kubeconfig and manifests in a single pass.
///
/// Done on Core so Periphery never has to resolve Komodo Variables.
/// One interpolator covers both strings, so the returned replacers
/// scrub secrets that came from either.
pub async fn interpolated_cluster(
  cluster: &Cluster,
) -> anyhow::Result<InterpolatedCluster> {
  let mut kubeconfig_contents =
    cluster.config.kubeconfig_contents.clone();
  let mut manifests = cluster.config.file_contents.clone();
  let mut secret_replacers = Vec::new();

  if !cluster.config.skip_secret_interp {
    let VariablesAndSecrets { variables, secrets } =
      get_variables_and_secrets()
        .await
        .context("Failed to get variables and secrets")?;
    let mut interpolator =
      Interpolator::new(Some(&variables), &secrets);
    interpolator
      .interpolate_string(&mut kubeconfig_contents)
      .context("Failed to interpolate variables into kubeconfig")?
      .interpolate_string(&mut manifests)
      .context("Failed to interpolate variables into manifests")?;
    secret_replacers =
      interpolator.secret_replacers.into_iter().collect();
  }

  Ok(InterpolatedCluster {
    target: ClusterTarget {
      kubeconfig_contents,
      kubeconfig_path: cluster.config.kubeconfig_path.clone(),
      context: cluster.config.context.clone(),
      proxy_url: cluster.config.proxy_url.clone(),
    },
    manifests,
    secret_replacers,
  })
}

/// Resolve where a Cluster's manifests come from into the flat spec
/// Periphery understands.
///
/// A linked Komodo Repo is flattened here, so Periphery never has to
/// know Repo resources exist - the same split used for the kubeconfig.
pub async fn cluster_manifest_source(
  cluster: &Cluster,
  manifests: String,
) -> anyhow::Result<ClusterManifestSource> {
  match cluster.config.manifest_source() {
    ClusterManifestSourceKind::Contents => {
      Ok(ClusterManifestSource::Contents(manifests))
    }

    ClusterManifestSourceKind::FilesOnHost => {
      if cluster.config.run_directory.is_empty() {
        return Err(anyhow!(
          "'files_on_host' needs a 'run_directory' to read from"
        ));
      }
      Ok(ClusterManifestSource::FilesOnHost {
        run_directory: cluster.config.run_directory.clone(),
        file_paths: cluster.config.file_paths.clone(),
      })
    }

    ClusterManifestSourceKind::LinkedRepo => {
      let mut repo =
        crate::resource::get::<Repo>(&cluster.config.linked_repo)
          .await
          .context("Failed to get the Cluster's linked Repo")?;
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
      Ok(ClusterManifestSource::Repo {
        args: (&repo).into(),
        git_token,
        reclone: cluster.config.reclone,
        // The Cluster says where in the repo its manifests live; the
        // Repo resource only says how to fetch it.
        run_directory: cluster.config.run_directory.clone(),
        file_paths: cluster.config.file_paths.clone(),
      })
    }

    ClusterManifestSourceKind::Repo => {
      let mut cluster = cluster.clone();
      let git_token = git_token(
        &cluster.config.git_provider,
        &cluster.config.git_account,
        |https| cluster.config.git_https = https,
      )
      .await
      .with_context(|| {
        format!(
          "Failed to get git token | {} | {}",
          cluster.config.git_provider, cluster.config.git_account
        )
      })?;
      Ok(ClusterManifestSource::Repo {
        args: (&cluster).into(),
        git_token,
        reclone: cluster.config.reclone,
        run_directory: cluster.config.run_directory.clone(),
        file_paths: cluster.config.file_paths.clone(),
      })
    }
  }
}

/// Just the connection target, for callers that don't apply manifests.
pub async fn cluster_target(
  cluster: &Cluster,
) -> anyhow::Result<ClusterTarget> {
  Ok(interpolated_cluster(cluster).await?.target)
}
