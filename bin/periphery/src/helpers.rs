use std::{
  fmt::Write, net::IpAddr, path::PathBuf, str::FromStr as _,
  sync::OnceLock, time::Duration,
};

use anyhow::Context;
use command::{
  CommandOptions, KomodoCommandMode,
  run_komodo_command_with_sanitization, run_standard_command,
};
use environment::write_env_file;
use interpolate::Interpolator;
use komodo_client::{
  entities::{
    EnvironmentVar, RepoExecutionArgs, RepoExecutionResponse,
    SearchCombinator, SshAuth, SystemCommand, TlsAuth,
    all_logs_success,
    config::ProviderAccount,
    credential_match::{PrefixCandidate, select_by_prefix},
    deployment::Conversion,
  },
  parsers::QUOTE_PATTERN,
};
use periphery_client::api::git::PeripheryRepoExecutionResponse;
use shell_escape::unix::escape;

use crate::config::periphery_config;

// ============
//  Formatting
// ============

pub fn format_extra_args(extra_args: &[String]) -> String {
  let args = extra_args.join(" ");
  if !args.is_empty() {
    format!(" {args}")
  } else {
    args
  }
}

pub fn push_extra_args(
  command: &mut String,
  extra_args: &[String],
) -> anyhow::Result<()> {
  for arg in extra_args {
    write!(command, " {arg}")
      .context("Failed to write extra args to command")?
  }
  Ok(())
}

pub fn format_labels(labels: &[EnvironmentVar]) -> String {
  labels
    .iter()
    .map(|p| {
      if p.value.starts_with(QUOTE_PATTERN)
        && p.value.ends_with(QUOTE_PATTERN)
      {
        // If the value already wrapped in quotes, don't wrap it again
        format!(" --label {}={}", p.variable, p.value)
      } else {
        format!(" --label {}=\"{}\"", p.variable, p.value)
      }
    })
    .collect::<Vec<_>>()
    .join("")
}

pub fn push_labels(
  command: &mut String,
  labels: &[EnvironmentVar],
) -> anyhow::Result<()> {
  for label in labels {
    if label.value.starts_with(QUOTE_PATTERN)
      && label.value.ends_with(QUOTE_PATTERN)
    {
      write!(command, " --label {}={}", label.variable, label.value)
    } else {
      write!(
        command,
        " --label {}=\"{}\"",
        label.variable, label.value
      )
    }
    .context("Failed to write labels to command")?;
  }
  Ok(())
}

pub fn push_conversions(
  command: &mut String,
  conversions: &[Conversion],
  flag: &str,
) -> anyhow::Result<()> {
  for Conversion { local, container } in conversions {
    write!(command, " {flag} {local}:{container}")
      .context("Failed to format conversions")?;
  }
  Ok(())
}

pub fn push_environment(
  command: &mut String,
  environment: &[EnvironmentVar],
) -> anyhow::Result<()> {
  for EnvironmentVar { variable, value } in environment {
    if value.starts_with(QUOTE_PATTERN)
      && value.ends_with(QUOTE_PATTERN)
    {
      write!(command, " --env {variable}={value}")
    } else {
      write!(command, " --env {variable}=\"{value}\"")
    }
    .context("Failed to format environment")?;
  }
  Ok(())
}

pub fn format_log_grep(
  terms: &[String],
  combinator: SearchCombinator,
  invert: bool,
) -> String {
  let maybe_invert = if invert { " -v" } else { Default::default() };
  match combinator {
    SearchCombinator::Or => {
      format!(
        "grep{maybe_invert} -E {}",
        escape(terms.join("|").into())
      )
    }
    SearchCombinator::And => {
      format!(
        "grep{maybe_invert} -P {}",
        escape(format!("^(?=.*{})", terms.join(")(?=.*")).into())
      )
    }
  }
}

// =====
//  Git
// =====

#[instrument(
  "PostRepoExecution",
  skip_all,
  fields(
    path = res.path.display().to_string(),
    env_file_path
  )
)]
pub async fn handle_post_repo_execution(
  mut res: RepoExecutionResponse,
  mut environment: Vec<EnvironmentVar>,
  env_file_path: &str,
  mut on_clone: Option<SystemCommand>,
  mut on_pull: Option<SystemCommand>,
  skip_secret_interp: bool,
  mut replacers: Vec<(String, String)>,
) -> anyhow::Result<PeripheryRepoExecutionResponse> {
  if !skip_secret_interp {
    let mut interpolotor =
      Interpolator::new(None, &periphery_config().secrets);
    interpolotor.interpolate_env_vars(&mut environment)?;
    if let Some(on_clone) = on_clone.as_mut() {
      interpolotor.interpolate_string(&mut on_clone.command)?;
    }
    if let Some(on_pull) = on_pull.as_mut() {
      interpolotor.interpolate_string(&mut on_pull.command)?;
    }
    replacers.extend(interpolotor.secret_replacers);
  }

  let env_file_path = write_env_file(
    &environment,
    &res.path,
    env_file_path,
    &mut res.logs,
  )
  .await;

  let mut res = PeripheryRepoExecutionResponse { res, env_file_path };

  if let Some(on_clone) = on_clone
    && !on_clone.is_none()
  {
    let path = res
      .res
      .path
      .join(on_clone.path)
      .components()
      .collect::<PathBuf>();
    if let Some(log) = run_komodo_command_with_sanitization(
      "On Clone",
      on_clone.command,
      CommandOptions::default().path(path.as_path()),
      if on_clone.shell_mode {
        KomodoCommandMode::Shell
      } else {
        KomodoCommandMode::Multiline
      },
      &replacers,
    )
    .await
    {
      res.res.logs.push(log);
      if !all_logs_success(&res.res.logs) {
        return Ok(res);
      }
    }
  }

  if let Some(on_pull) = on_pull
    && !on_pull.is_none()
  {
    let path = res
      .res
      .path
      .join(on_pull.path)
      .components()
      .collect::<PathBuf>();
    if let Some(log) = run_komodo_command_with_sanitization(
      "On Pull",
      on_pull.command,
      CommandOptions::default().path(path.as_path()),
      if on_pull.shell_mode {
        KomodoCommandMode::Shell
      } else {
        KomodoCommandMode::Multiline
      },
      &replacers,
    )
    .await
    {
      res.res.logs.push(log);
    }
  }

  Ok(res)
}

// =======
//  Token
// =======

pub fn git_token_simple(
  domain: &str,
  account_username: &str,
) -> anyhow::Result<&'static str> {
  periphery_config()
    .git_providers
    .iter()
    .find(|provider| provider.domain == domain)
    .and_then(|provider| {
      provider.accounts.iter().find(|account| account.username == account_username).map(|account| account.token.as_str())
    })
    .with_context(|| format!("Did not find token in config for git account {account_username} | domain {domain}"))
}

pub fn git_token(
  core_token: Option<String>,
  args: &RepoExecutionArgs,
) -> anyhow::Result<Option<String>> {
  if core_token.is_some() {
    return Ok(core_token);
  }
  let Some(account) = &args.account else {
    // No account named on the resource. Before giving up and cloning
    // anonymously - which is what happened unconditionally until now -
    // see whether a configured account covers this repo's path.
    return git_token_by_prefix(&args.provider, args.repo.as_deref());
  };
  let token = git_token_simple(&args.provider, account)?;
  // An account configured with a blank token is not a credential. An
  // ssh-only account is exactly that shape, and passing Some("") down
  // makes every downstream consumer treat the empty string as a secret
  // to find and redact.
  Ok(non_empty_token(token))
}

/// `Some` only when there is an actual token.
fn non_empty_token(token: &str) -> Option<String> {
  (!token.is_empty()).then(|| token.to_string())
}

/// Fallback for a resource that names no git account: pick the
/// configured account whose `path_prefix` is the longest segment-wise
/// match for the repo path.
///
/// Returns `Ok(None)` rather than an error when nothing matches, because
/// no match is the normal case - it means "clone anonymously", exactly
/// as before this existed. An AMBIGUOUS match is different: it is a
/// configuration mistake, and resolving it by guessing would let a
/// config reorder silently change which credential reaches a remote. So
/// that is surfaced as an error the operator can act on.
fn git_token_by_prefix(
  domain: &str,
  repo_path: Option<&str>,
) -> anyhow::Result<Option<String>> {
  let Some(repo_path) = repo_path else {
    return Ok(None);
  };
  let Some(provider) = periphery_config()
    .git_providers
    .iter()
    .find(|provider| provider.domain == domain)
  else {
    return Ok(None);
  };
  let candidates = provider
    .accounts
    .iter()
    .map(|account| PrefixCandidate {
      username: &account.username,
      path_prefix: &account.path_prefix,
    })
    .collect::<Vec<_>>();
  let Some(username) = select_by_prefix(&candidates, repo_path)
    .with_context(|| {
      format!(
        "Failed to select a git account for {domain}/{repo_path}"
      )
    })?
  else {
    return Ok(None);
  };
  let token = git_token_simple(domain, username)?;
  Ok(non_empty_token(token))
}

/// Fill in `args.ssh` from this Periphery's git provider config, when
/// the account backing this remote has an ssh key.
///
/// Only fills when it is empty, so material Core resolved and sent takes
/// precedence - Core knows about DB-stored accounts that Periphery's
/// config does not.
///
/// Account selection mirrors `git_token`: the account named on the
/// resource, else the longest matching path prefix. Resolving the key
/// from a different account than the token would be a confusing way to
/// half-authenticate.
pub fn with_git_ssh(
  mut args: RepoExecutionArgs,
) -> anyhow::Result<RepoExecutionArgs> {
  if args.ssh.is_some() {
    return Ok(args);
  }
  let Some(account) = resolve_account(&args)? else {
    return Ok(args);
  };
  if account.ssh_private_key.trim().is_empty() {
    return Ok(args);
  }
  args.ssh = Some(SshAuth {
    private_key: account.ssh_private_key.clone(),
    known_hosts: account.ssh_known_hosts.clone(),
    accept_new_host_keys: account.ssh_accept_new_host_keys,
  });
  Ok(args)
}

/// Fill in `args.tls` from this Periphery's git provider config.
///
/// Same account selection and same precedence rule as `with_git_ssh`:
/// material Core sent wins, then the account named on the resource, then
/// the longest matching path prefix.
pub fn with_git_tls(
  mut args: RepoExecutionArgs,
) -> anyhow::Result<RepoExecutionArgs> {
  if args.tls.is_some() {
    return Ok(args);
  }
  let Some(account) = resolve_account(&args)? else {
    return Ok(args);
  };
  if account.tls_client_cert.trim().is_empty()
    && account.tls_client_key.trim().is_empty()
    && account.tls_ca_bundle.trim().is_empty()
  {
    return Ok(args);
  }
  args.tls = Some(TlsAuth {
    client_cert: account.tls_client_cert.clone(),
    client_key: account.tls_client_key.clone(),
    ca_bundle: account.tls_ca_bundle.clone(),
  });
  Ok(args)
}

/// The provider account backing this remote: the one named on the
/// resource, else the longest matching path prefix.
///
/// Shared so the ssh key, the TLS material and the token can never be
/// resolved from DIFFERENT accounts - a half-authenticated request built
/// from two identities is a confusing failure to debug.
fn resolve_account(
  args: &RepoExecutionArgs,
) -> anyhow::Result<Option<&'static ProviderAccount>> {
  let Some(provider) = periphery_config()
    .git_providers
    .iter()
    .find(|provider| provider.domain == args.provider)
  else {
    return Ok(None);
  };
  let username = match &args.account {
    Some(account) => Some(account.clone()),
    None => {
      let Some(repo_path) = args.repo.as_deref() else {
        return Ok(None);
      };
      let candidates = provider
        .accounts
        .iter()
        .map(|account| PrefixCandidate {
          username: &account.username,
          path_prefix: &account.path_prefix,
        })
        .collect::<Vec<_>>();
      select_by_prefix(&candidates, repo_path)
        .with_context(|| {
          format!(
            "Failed to select a git account for {}/{repo_path}",
            args.provider
          )
        })?
        .map(str::to_string)
    }
  };
  let Some(username) = username else {
    return Ok(None);
  };
  Ok(
    provider
      .accounts
      .iter()
      .find(|account| account.username == username),
  )
}

pub fn registry_token(
  domain: &str,
  account_username: &str,
) -> anyhow::Result<&'static str> {
  periphery_config()
    .image_registries
    .iter()
    .find(|registry| registry.domain == domain)
    .and_then(|registry| {
      registry.accounts.iter().find(|account| account.username == account_username).map(|account| account.token.as_str())
    })
    .with_context(|| format!("did not find token in config for docker registry account {account_username} | domain {domain}"))
}

// ====================
//  Public IP over DNS
// ====================

type OpenDNSResolver = hickory_resolver::TokioResolver;

fn opendns_resolver() -> &'static OpenDNSResolver {
  static OPENDNS_RESOLVER: OnceLock<OpenDNSResolver> =
    OnceLock::new();
  OPENDNS_RESOLVER.get_or_init(|| {
    // OpenDNS resolver ipv4s.
    let name_servers = [
      IpAddr::from_str("208.67.220.220").unwrap(),
      IpAddr::from_str("208.67.222.222").unwrap(),
    ]
    .into_iter()
    .map(hickory_resolver::config::NameServerConfig::udp_and_tcp)
    .collect();

    hickory_resolver::Resolver::builder_with_config(
      hickory_resolver::config::ResolverConfig::from_parts(
        None,
        vec![],
        name_servers,
      ),
      hickory_resolver::net::runtime::TokioRuntimeProvider::default(),
    )
    .build()
    .expect("Failed to build OpenDNS resolver")
  })
}

/// Includes 1s timeout
pub async fn resolve_host_public_ip() -> anyhow::Result<String> {
  tokio::time::timeout(Duration::from_secs(1), async {
    opendns_resolver()
      .lookup_ip("myip.opendns.com.")
      .await
      .context(
        "Failed to query OpenDNS resolvers for host public IP",
      )?
      .iter()
      .map(|ip| ip.to_string())
      .next()
      .context("OpenDNS call for public IP didn't return anything")
  })
  .await
  .context("OpenDNS call for public IP timed out")
  .flatten()
}

// =====
//  SSL
// =====

pub async fn ensure_ssl_certs() {
  let config = periphery_config();
  if !config.ssl_cert_file().is_file()
    || !config.ssl_key_file().is_file()
  {
    generate_self_signed_ssl_certs().await
  }
}

#[instrument("GenerateSslCerts")]
async fn generate_self_signed_ssl_certs() {
  info!("Generating certs...");

  let config = periphery_config();

  let ssl_key_file = config.ssl_key_file();
  let ssl_cert_file = config.ssl_cert_file();

  // ensure cert folders exist
  if let Some(parent) = ssl_key_file.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  if let Some(parent) = ssl_cert_file.parent() {
    let _ = std::fs::create_dir_all(parent);
  }

  let key_path = ssl_key_file.display();
  let cert_path = ssl_cert_file.display();

  let command = format!(
    "openssl req -x509 -newkey rsa:4096 -keyout {key_path} -out {cert_path} -sha256 -days 3650 -nodes -subj \"/C=XX/CN=periphery\""
  );
  let log =
    run_standard_command(&command, CommandOptions::default()).await;

  if log.success() {
    info!("✅ SSL Certs generated");
  } else {
    panic!(
      "🚨 Failed to generate SSL Certs | stdout: {} | stderr: {}",
      log.stdout, log.stderr
    );
  }
}
