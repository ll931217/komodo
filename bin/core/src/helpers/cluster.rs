use anyhow::Context;
use interpolate::Interpolator;
use komodo_client::entities::cluster::Cluster;
use periphery_client::api::cluster::ClusterTarget;

use super::query::{VariablesAndSecrets, get_variables_and_secrets};

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

/// Just the connection target, for callers that don't apply manifests.
pub async fn cluster_target(
  cluster: &Cluster,
) -> anyhow::Result<ClusterTarget> {
  Ok(interpolated_cluster(cluster).await?.target)
}
