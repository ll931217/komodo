use anyhow::Context;
use interpolate::Interpolator;
use komodo_client::entities::cluster::{
  Cluster, ClusterConfig, ManifestPolicy,
};
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

/// Err when the Cluster's kind policy forbids operating on `kind`.
///
/// Thin wrapper over [ManifestPolicy::check_kind], which is where the
/// rules live now: Periphery enforces the same policy against the
/// materialized objects, and two copies of a blast-radius rule is one
/// copy too many.
/// Object names reach the kubectl command line on Periphery, so
/// anything outside the Kubernetes name charset is refused here.
pub fn check_object_name(name: &str) -> anyhow::Result<()> {
  if !name.is_empty()
    && name.chars().all(|c| {
      c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
    })
  {
    Ok(())
  } else {
    anyhow::bail!("'{name}' is not a valid Kubernetes object name")
  }
}

pub fn check_kind_allowed(
  config: &ClusterConfig,
  kind: &str,
) -> anyhow::Result<()> {
  ManifestPolicy::from(config).check_kind(kind)
}

/// The first kind declared in `manifests` that the Cluster's kind
/// policy forbids, if any.
///
/// The same shallow `kind:` line scan the cluster-scoped check uses,
/// and the same reasoning: it errs toward refusing, which is the safe
/// direction for a blast-radius control.
pub fn forbidden_manifest_kind(
  manifests: &str,
  config: &ClusterConfig,
) -> Option<String> {
  manifests.lines().find_map(|line| {
    let kind =
      line.trim().strip_prefix("kind:")?.trim().trim_matches('"');
    check_kind_allowed(config, kind)
      .is_err()
      .then(|| kind.to_string())
  })
}

#[cfg(test)]
mod kind_policy_tests {
  use komodo_client::entities::cluster::ClusterConfig;

  use super::{check_kind_allowed, forbidden_manifest_kind};

  fn config(exclude: &[&str], include: &[&str]) -> ClusterConfig {
    ClusterConfig {
      exclude_kinds: exclude.iter().map(|s| s.to_string()).collect(),
      include_kinds: include.iter().map(|s| s.to_string()).collect(),
      ..Default::default()
    }
  }

  #[test]
  fn no_policy_allows_everything() {
    assert!(check_kind_allowed(&config(&[], &[]), "Secret").is_ok());
  }

  #[test]
  fn plural_and_case_do_not_matter() {
    let config = config(&["Secret"], &[]);
    for kind in ["Secret", "secret", "secrets", " SECRETS "] {
      assert!(
        check_kind_allowed(&config, kind).is_err(),
        "{kind} should be excluded"
      );
    }
  }

  /// The case a single trailing-`s` strip got wrong: the plural of a
  /// kind already ending in `s` adds `es`, so `Ingress` and
  /// `Ingresses` share no strip-one-`s` normal form. An exclude
  /// written the way `kubectl get` spells it has to still block.
  #[test]
  fn double_s_kinds_match_their_real_plural() {
    for (pattern, kind) in [
      ("Ingresses", "Ingress"),
      ("Ingress", "Ingresses"),
      ("StorageClasses", "StorageClass"),
      ("StorageClass", "StorageClasses"),
      ("PriorityClasses", "PriorityClass"),
      ("NetworkPolicies", "NetworkPolicy"),
      ("NetworkPolicy", "NetworkPolicies"),
    ] {
      let config = config(&[pattern], &[]);
      assert!(
        check_kind_allowed(&config, kind).is_err(),
        "exclude '{pattern}' should block kind '{kind}'"
      );
    }
  }

  #[test]
  fn unrelated_kinds_still_pass() {
    let config = config(&["Ingress"], &[]);
    for kind in ["Deployment", "Service", "ConfigMap", "Secret"] {
      assert!(
        check_kind_allowed(&config, kind).is_ok(),
        "{kind} should not be caught by an Ingress exclude"
      );
    }
  }

  #[test]
  fn wildcards_match() {
    let config = config(&["*role*"], &[]);
    assert!(
      check_kind_allowed(&config, "ClusterRoleBinding").is_err()
    );
    assert!(check_kind_allowed(&config, "Deployment").is_ok());
  }

  #[test]
  fn include_is_an_allow_list_and_an_override() {
    // Allow-list: anything unlisted is refused.
    let allow_list = config(&[], &["Deployment", "Service"]);
    assert!(check_kind_allowed(&allow_list, "Deployment").is_ok());
    assert!(check_kind_allowed(&allow_list, "Secret").is_err());

    // Override: the narrow include wins over the broad exclude.
    let override_pair = config(&["*"], &["ConfigMap"]);
    assert!(check_kind_allowed(&override_pair, "ConfigMap").is_ok());
    assert!(check_kind_allowed(&override_pair, "Secret").is_err());
  }

  #[test]
  fn manifest_scan_finds_the_forbidden_kind() {
    let config = config(&["Secret"], &[]);
    let manifests = "apiVersion: v1\nkind: ConfigMap\n---\napiVersion: v1\nkind: Secret\n";
    assert_eq!(
      forbidden_manifest_kind(manifests, &config),
      Some(String::from("Secret"))
    );
    assert_eq!(
      forbidden_manifest_kind("kind: ConfigMap\n", &config),
      None
    );
  }
}
