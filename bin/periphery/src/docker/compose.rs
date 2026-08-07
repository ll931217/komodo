use std::time::Duration;

use anyhow::{Context, anyhow};
use command::{CommandOptions, run_komodo_standard_command};
use komodo_client::entities::{
  ResourceTargetVariant,
  stack::*,
  tracking::{TRACKING_LABEL, TrackingId},
};
use serde::{Deserialize, Serialize};

use crate::config::periphery_config;

pub fn docker_compose() -> &'static str {
  if periphery_config().legacy_compose_cli {
    "docker-compose"
  } else {
    "docker compose"
  }
}

pub async fn list_compose_projects()
-> anyhow::Result<Vec<ComposeProject>> {
  let docker_compose = docker_compose();
  let res = run_komodo_standard_command(
    "List Projects",
    format!("{docker_compose} ls --all --format json"),
    CommandOptions::default().timeout(Duration::from_secs(1)),
  )
  .await;

  if !res.success {
    return Err(anyhow!("{}", res.combined()).context(format!(
      "Failed to list compose projects using {docker_compose} ls"
    )));
  }

  let mut res =
    serde_json::from_str::<Vec<DockerComposeLsItem>>(&res.stdout)
      .with_context(|| res.stdout.clone())
      .with_context(|| {
        format!(
          "Failed to parse '{docker_compose} ls' response from json"
        )
      })?
      .into_iter()
      .filter(|item| !item.name.is_empty())
      .map(|item| ComposeProject {
        name: item.name,
        status: item.status,
        compose_files: item
          .config_files
          .split(',')
          .map(str::to_string)
          .collect(),
      })
      .collect::<Vec<_>>();

  res.sort_by(|a, b| {
    a.status.cmp(&b.status).then_with(|| a.name.cmp(&b.name))
  });

  Ok(res)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerComposeLsItem {
  #[serde(default, alias = "Name")]
  pub name: String,
  #[serde(alias = "Status")]
  pub status: Option<String>,
  /// Comma seperated list of paths
  #[serde(default, alias = "ConfigFiles")]
  pub config_files: String,
}

pub fn parse_compose_services(
  raw_config: &str,
  project_name: &str,
  services: &mut Vec<StackServiceNames>,
) -> anyhow::Result<()> {
  let compose = serde_yaml_ng::from_str::<ComposeFile>(raw_config)
    .context("Failed to parse compose contents")?;

  for (
    service_name,
    ComposeService {
      container_name,
      deploy,
      image,
    },
  ) in compose.services
  {
    let image = image.unwrap_or_default();
    match deploy {
      Some(ComposeServiceDeploy {
        replicas: Some(replicas),
      }) if replicas > 1 => {
        for i in 1..1 + replicas {
          services.push(StackServiceNames {
            container_name: format!(
              "{project_name}-{service_name}-{i}"
            ),
            service_name: format!("{service_name}-{i}"),
            image: image.clone(),
            image_digest: None,
          });
        }
      }
      _ => {
        services.push(StackServiceNames {
          container_name: container_name.unwrap_or_else(|| {
            format!("{project_name}-{service_name}")
          }),
          service_name,
          image,
          image_digest: None,
        });
      }
    }
  }

  Ok(())
}

/// The file name of the generated compose override which stamps
/// [TRACKING_LABEL] onto every service of a Stack. Written into the
/// run directory before `compose up`, removed after.
pub const TRACKING_OVERRIDE_FILE: &str =
  ".komodo-tracking.compose.yaml";

/// Build a compose override which stamps [TRACKING_LABEL] on every
/// service found in the merged config, so containers a Stack creates
/// declare their owner instead of being matched by project name alone.
///
/// `raw_config` is the stdout of `docker compose config`, so the service
/// keys here are the real (post-merge, post-interpolation) service names.
pub fn tracking_override_contents(
  raw_config: &str,
  stack_id: &str,
  project_name: &str,
) -> anyhow::Result<String> {
  let compose = serde_yaml_ng::from_str::<ComposeFile>(raw_config)
    .context("Failed to parse compose contents")?;

  // Deterministic ordering, so a redeploy with no changes
  // doesn't rewrite the file differently each time.
  let mut service_names =
    compose.services.into_keys().collect::<Vec<_>>();
  service_names.sort();

  let mut contents = String::from("services:\n");
  for service_name in service_names {
    let tracking = TrackingId::new(
      ResourceTargetVariant::Stack,
      stack_id,
      format!("{project_name}/{service_name}"),
    );
    // ponytail: hand written yaml - the shape is three fixed lines and
    // the only interpolated values are a service name, an id, and a
    // project name. Reach for a serializer if this grows.
    contents.push_str(&format!(
      "  {}:\n    labels:\n      {}: \"{}\"\n",
      serde_yaml_ng::to_string(&service_name)
        .context("Failed to serialize service name")?
        .trim(),
      TRACKING_LABEL,
      tracking.to_label_value()
    ));
  }

  Ok(contents)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tracking_override_covers_every_service() {
    let config = r#"
services:
  api:
    image: nginx
  db:
    image: postgres
"#;
    let contents =
      tracking_override_contents(config, "abc123", "my-proj")
        .unwrap();
    assert_eq!(
      contents,
      "services:\n  api:\n    labels:\n      komodo.tracking-id: \"Stack/abc123/my-proj/api\"\n  db:\n    labels:\n      komodo.tracking-id: \"Stack/abc123/my-proj/db\"\n"
    );
    // The generated override must itself be valid compose yaml.
    serde_yaml_ng::from_str::<ComposeFile>(&contents).unwrap();
  }
}
