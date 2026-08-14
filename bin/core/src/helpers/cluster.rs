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
  /// (secret value, replacement) pairs, so command output can be
  /// scrubbed before it is stored in an Update or shown to a user.
  pub secret_replacers: Vec<(String, String)>,
}

/// Interpolate a Cluster's kubeconfig.
///
/// Done on Core so Periphery never has to resolve Komodo Variables.
/// An Application's manifests are interpolated separately, against
/// their own resource's `skip_secret_interp`.
pub async fn interpolated_cluster(
  cluster: &Cluster,
) -> anyhow::Result<InterpolatedCluster> {
  let mut kubeconfig_contents =
    cluster.config.kubeconfig_contents.clone();
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
      .context("Failed to interpolate variables into kubeconfig")?;
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
    secret_replacers,
  })
}

/// Just the connection target, for callers that don't apply manifests
/// and have nothing to scrub.
///
/// If the caller logs anything derived from the request - an error
/// raised before Periphery answers, say - use
/// [cluster_target_and_replacers] instead: the target is built from
/// interpolated config, so dropping the replacers drops the only means
/// of keeping secrets out of that log.
pub async fn cluster_target(
  cluster: &Cluster,
) -> anyhow::Result<ClusterTarget> {
  Ok(interpolated_cluster(cluster).await?.target)
}

/// The connection target plus the replacers that scrub secrets out of
/// anything logged about it.
pub async fn cluster_target_and_replacers(
  cluster: &Cluster,
) -> anyhow::Result<(ClusterTarget, Vec<(String, String)>)> {
  let InterpolatedCluster {
    target,
    secret_replacers,
    ..
  } = interpolated_cluster(cluster).await?;
  Ok((target, secret_replacers))
}
