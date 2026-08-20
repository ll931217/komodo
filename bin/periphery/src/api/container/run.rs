use std::fmt::Write;

use anyhow::Context;
use command::{
  CommandOptions, KomodoCommandMode,
  run_komodo_command_with_sanitization,
};
use formatting::format_serror;
use interpolate::Interpolator;
use komodo_client::entities::{
  ResourceTargetVariant,
  deployment::{
    Deployment, DeploymentConfig, DeploymentImage, RestartMode,
    conversions_from_str, extract_registry_domain,
  },
  environment_vars_from_str,
  tracking::{TRACKING_LABEL, TrackingId},
  update::Log,
};
use mogh_resolver::Resolve;
use periphery_client::api::container::{
  RemoveContainer, RunContainer, RunContainerResponse,
};
use tracing::Instrument;

use crate::{
  config::periphery_config,
  docker::{docker_login, pull_image},
  helpers::{
    push_conversions, push_environment, push_extra_args, push_labels,
  },
};

impl Resolve<crate::api::Args> for RunContainer {
  #[instrument(
    "DeployContainer",
    skip_all,
    fields(
      id = args.id.to_string(),
      core = args.core,
      deployment = &self.deployment.name,
      stop_signal = format!("{:?}", self.stop_signal),
      stop_time = self.stop_time,
    )
  )]
  async fn resolve(
    self,
    args: &crate::api::Args,
  ) -> anyhow::Result<RunContainerResponse> {
    let RunContainer {
      mut deployment,
      stop_signal,
      stop_time,
      registry_token,
      mut replacers,
    } = self;

    let mut interpolator =
      Interpolator::new(None, &periphery_config().secrets);
    interpolator.interpolate_deployment(&mut deployment)?;
    replacers.extend(interpolator.secret_replacers);

    let image = if let DeploymentImage::Image { image } =
      &deployment.config.image
    {
      if image.is_empty() {
        return Ok(RunContainerResponse::Logs(vec![Log::error(
          "Get Image",
          String::from("Deployment does not have image attached"),
        )]));
      }
      image
    } else {
      return Ok(RunContainerResponse::Logs(vec![Log::error(
        "Get Image",
        String::from("Deployment does not have image attached"),
      )]));
    };

    if let Err(e) = docker_login(
      &extract_registry_domain(image)?,
      &deployment.config.image_registry_account,
      registry_token.as_deref(),
    )
    .await
    {
      return Ok(RunContainerResponse::Logs(vec![Log::error(
        "Docker Login",
        format_serror(
          &e.context("Failed to login to docker registry").into(),
        ),
      )]));
    }

    let _ = pull_image(image).await;
    debug!("image pulled");

    // Remove the existing container under the previously deployed name,
    // in case the name configuration changed since the last deploy.
    let _ = (RemoveContainer {
      name: deployment.deployed_name().to_string(),
      signal: stop_signal,
      time: stop_time,
    })
    .resolve(args)
    .await;
    debug!("container stopped and removed");

    let command = docker_run_command(&deployment, image)
      .context("Unable to generate valid docker run command")?;

    let mut logs = Vec::new();

    // A Deployment has no repo, so a hook's `path` is the working
    // directory as given rather than relative to a checkout.
    let hook_root = std::path::PathBuf::from("/");

    if let Some(log) = crate::helpers::run_hook(
      "Pre Deploy",
      &deployment.config.pre_deploy,
      &hook_root,
      None,
      &replacers,
    )
    .await
    {
      let success = log.success;
      logs.push(log);
      // A failed pre-deploy stops the deploy: the point of running
      // something first is that what follows depends on it.
      if !success {
        return Ok(RunContainerResponse::Logs(logs));
      }
    }

    let span = info_span!("ExecuteDockerRun");
    let Some(log) = run_komodo_command_with_sanitization(
      "Docker Run",
      command,
      CommandOptions::default(),
      KomodoCommandMode::Shell,
      &replacers,
    )
    .instrument(span)
    .await
    else {
      // The none case is only for empty command,
      // this won't be the case given it is populated above.
      unreachable!()
    };

    let deployed = log.success;
    logs.push(log);

    if let Some(log) = crate::helpers::run_hook(
      if deployed {
        "Post Deploy"
      } else {
        "On Deploy Fail"
      },
      if deployed {
        &deployment.config.post_deploy
      } else {
        &deployment.config.on_deploy_fail
      },
      &hook_root,
      None,
      &replacers,
    )
    .await
    {
      logs.push(log);
    }

    Ok(RunContainerResponse::Logs(logs))
  }
}

fn docker_run_command(
  deployment: &Deployment,
  image: &str,
) -> anyhow::Result<String> {
  let name = deployment.custom_name();
  let DeploymentConfig {
    volumes,
    ports,
    network,
    command,
    restart,
    environment,
    labels,
    extra_args,
    ..
  } = &deployment.config;
  let mut res =
    format!("docker run -d --name {name} --network {network}");

  push_conversions(
    &mut res,
    &conversions_from_str(ports).context("Invalid ports")?,
    "-p",
  )?;

  push_conversions(
    &mut res,
    &conversions_from_str(volumes).context("Invalid volumes")?,
    "-v",
  )?;

  push_environment(
    &mut res,
    &environment_vars_from_str(environment)
      .context("Invalid environment")?,
  )?;

  push_restart(&mut res, restart)?;

  push_labels(
    &mut res,
    &environment_vars_from_str(labels).context("Invalid labels")?,
  )?;

  push_extra_args(&mut res, extra_args)?;

  // Stamped last so user labels / extra args can't spoof ownership.
  write!(
    &mut res,
    " --label {TRACKING_LABEL}=\"{}\"",
    TrackingId::new(
      ResourceTargetVariant::Deployment,
      &deployment.id,
      name
    )
    .to_label_value()
  )?;

  write!(&mut res, " {image}")?;

  if !command.is_empty() {
    write!(&mut res, " {command}")?;
  }

  Ok(res)
}

fn push_restart(
  command: &mut String,
  restart: &RestartMode,
) -> anyhow::Result<()> {
  let restart = match restart {
    RestartMode::OnFailure => "on-failure:10",
    _ => restart.as_ref(),
  };
  write!(command, " --restart {restart}")
    .context("Failed to write restart mode")
}
