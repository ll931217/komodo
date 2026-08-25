use anyhow::anyhow;
use interpolate::Interpolator;
use komodo_client::{
  api::execute::*,
  entities::{
    SwarmOrServer,
    permission::PermissionLevel,
    repo::Repo,
    server::Server,
    stack::{Stack, StackActionState},
    update::{Log, Update},
    user::User,
  },
};
use periphery_client::api::compose::*;

use crate::{
  helpers::{
    periphery_client,
    query::{VariablesAndSecrets, get_variables_and_secrets},
    update::update_update,
  },
  monitor::refresh_server_cache,
  periphery::PeripheryClient,
  state::action_states,
};

use super::setup_stack_execution;

pub trait ExecuteCompose {
  type Extras;

  /// A Vec rather than one Log because DestroyStack can run delete
  /// hooks around the `compose down`, and folding those into a single
  /// log would lose which stage failed.
  async fn execute(
    periphery: PeripheryClient,
    stack: Stack,
    services: Vec<String>,
    extras: Self::Extras,
  ) -> anyhow::Result<Vec<Log>>;
}

pub async fn execute_compose<T: ExecuteCompose>(
  stack: &str,
  services: Vec<String>,
  user: &User,
  set_in_progress: impl Fn(&mut StackActionState),
  update: Update,
  extras: T::Extras,
) -> anyhow::Result<Update> {
  let (stack, swarm_or_server) = setup_stack_execution(
    stack,
    user,
    PermissionLevel::Execute.into(),
  )
  .await?;

  let SwarmOrServer::Server(server) = swarm_or_server else {
    return Err(anyhow!(
      "Compose executions (Start, Stop, Restart) should not be called for Stack in Swarm Mode"
    ));
  };

  execute_compose_with_stack_and_server::<T>(
    stack,
    server,
    services,
    set_in_progress,
    update,
    extras,
  )
  .await
}

pub async fn execute_compose_with_stack_and_server<
  T: ExecuteCompose,
>(
  stack: Stack,
  server: Server,
  services: Vec<String>,
  set_in_progress: impl Fn(&mut StackActionState),
  mut update: Update,
  extras: T::Extras,
) -> anyhow::Result<Update> {
  // get the action state for the stack (or insert default).
  let action_state =
    action_states().stack.get_or_insert_default(&stack.id).await;

  // Will check to ensure stack not already busy before updating, and return Err if so.
  // The returned guard will set the action state back to default when dropped.
  let action_guard = action_state.update(set_in_progress)?;

  // Send update here for UI to recheck action state
  update_update(update.clone()).await?;

  let periphery = periphery_client(&server).await?;

  if !services.is_empty() {
    update.logs.push(Log::simple(
      "Service/s",
      format!(
        "Execution requested for Stack service/s {}",
        services.join(", ")
      ),
    ))
  }

  update
    .logs
    .extend(T::execute(periphery, stack, services, extras).await?);

  // Ensure cached stack state up to date by updating server cache
  refresh_server_cache(&server, true).await;

  update.finalize();

  // Drop action guard before updating
  // clients to requery action state
  drop(action_guard);
  update_update(update.clone()).await?;

  Ok(update)
}

fn service_args(services: &[String]) -> String {
  if !services.is_empty() {
    format!(" {}", services.join(" "))
  } else {
    String::new()
  }
}

impl ExecuteCompose for StartStack {
  type Extras = ();
  async fn execute(
    periphery: PeripheryClient,
    stack: Stack,
    services: Vec<String>,
    _: Self::Extras,
  ) -> anyhow::Result<Vec<Log>> {
    let service_args = service_args(&services);
    periphery
      .request(ComposeExecution {
        project: stack.project_name(false),
        command: format!("start{service_args}"),
      })
      .await
      .map(|log| vec![log])
  }
}

impl ExecuteCompose for RestartStack {
  type Extras = ();
  async fn execute(
    periphery: PeripheryClient,
    stack: Stack,
    services: Vec<String>,
    _: Self::Extras,
  ) -> anyhow::Result<Vec<Log>> {
    let service_args = service_args(&services);
    periphery
      .request(ComposeExecution {
        project: stack.project_name(false),
        command: format!("restart{service_args}"),
      })
      .await
      .map(|log| vec![log])
  }
}

impl ExecuteCompose for PauseStack {
  type Extras = ();
  async fn execute(
    periphery: PeripheryClient,
    stack: Stack,
    services: Vec<String>,
    _: Self::Extras,
  ) -> anyhow::Result<Vec<Log>> {
    let service_args = service_args(&services);
    periphery
      .request(ComposeExecution {
        project: stack.project_name(false),
        command: format!("pause{service_args}"),
      })
      .await
      .map(|log| vec![log])
  }
}

impl ExecuteCompose for UnpauseStack {
  type Extras = ();
  async fn execute(
    periphery: PeripheryClient,
    stack: Stack,
    services: Vec<String>,
    _: Self::Extras,
  ) -> anyhow::Result<Vec<Log>> {
    let service_args = service_args(&services);
    periphery
      .request(ComposeExecution {
        project: stack.project_name(false),
        command: format!("unpause{service_args}"),
      })
      .await
      .map(|log| vec![log])
  }
}

impl ExecuteCompose for StopStack {
  type Extras = Option<i32>;
  async fn execute(
    periphery: PeripheryClient,
    stack: Stack,
    services: Vec<String>,
    timeout: Self::Extras,
  ) -> anyhow::Result<Vec<Log>> {
    let service_args = service_args(&services);
    let maybe_timeout = maybe_timeout(timeout);
    periphery
      .request(ComposeExecution {
        project: stack.project_name(false),
        command: format!("stop{maybe_timeout}{service_args}"),
      })
      .await
      .map(|log| vec![log])
  }
}

impl ExecuteCompose for DestroyStack {
  type Extras = (Option<i32>, bool);
  async fn execute(
    periphery: PeripheryClient,
    stack: Stack,
    services: Vec<String>,
    (timeout, remove_orphans): Self::Extras,
  ) -> anyhow::Result<Vec<Log>> {
    // Without hooks the destroy is still just a command, so it keeps
    // taking the ComposeExecution path. That matters for reach: an
    // agent predating ComposeDown cannot deserialize the new request
    // at all, and every hook-less Stack would otherwise stop being
    // destroyable the moment Core upgraded.
    if stack.config.pre_delete.is_none()
      && stack.config.post_delete.is_none()
    {
      let service_args = service_args(&services);
      let maybe_timeout = maybe_timeout(timeout);
      let maybe_remove_orphans = if remove_orphans {
        " --remove-orphans"
      } else {
        ""
      };
      return periphery
        .request(ComposeExecution {
          project: stack.project_name(false),
          command: format!(
            "down{maybe_timeout}{maybe_remove_orphans}{service_args}"
          ),
        })
        .await
        .map(|log| vec![log]);
    }

    let mut stack = stack;

    // Only to locate the run directory the hooks execute in -
    // Periphery derives the path from it and clones nothing.
    let repo = if !stack.config.files_on_host
      && !stack.config.linked_repo.is_empty()
    {
      Some(
        crate::resource::get::<Repo>(&stack.config.linked_repo)
          .await?,
      )
    } else {
      None
    };

    // The hook commands are config like any other, so a Variable or
    // secret in one has to resolve before it reaches the host, and its
    // value has to be scrubbed from the log that comes back.
    let mut logs = Vec::new();
    let replacers = if stack.config.skip_secret_interp {
      Vec::new()
    } else {
      let VariablesAndSecrets { variables, secrets } =
        get_variables_and_secrets().await?;
      let mut interpolator =
        Interpolator::new(Some(&variables), &secrets);
      interpolator.interpolate_stack(&mut stack)?;
      interpolator.push_logs(&mut logs);
      interpolator.secret_replacers.into_iter().collect()
    };

    let res = periphery
      .request(ComposeDown {
        stack,
        repo,
        services,
        timeout,
        remove_orphans,
        replacers,
      })
      .await?;
    logs.extend(res.logs);
    Ok(logs)
  }
}

pub fn maybe_timeout(timeout: Option<i32>) -> String {
  if let Some(timeout) = timeout {
    format!(" --timeout {timeout}")
  } else {
    String::new()
  }
}
